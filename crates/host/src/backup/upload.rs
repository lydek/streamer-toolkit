//! Restore uploads: the backup file comes in parts of at most 16 MB into `cache/restore-upload-<id>.part`, so neither
//! one request body nor memory bounds its size, and the admin UI can show progress and cancel. One upload at a time:
//! starting another drops the earlier one. The file is removed once checked, on cancel, and on the next start.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    Json,
    body::Bytes,
    extract::{Path as UrlPath, Query, State, rejection::BytesRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use utoipa::{IntoParams, ToSchema};

use super::{
    BackupError, BackupState, MAX_ARCHIVE_BYTES, RestorePreview, room, stage, stage_failed,
};
use crate::admin::{ErrorBody, error};

/// The largest part one request may carry.
pub(super) const PART_MAX_BYTES: usize = 16 * 1024 * 1024;
const PREFIX: &str = "restore-upload-";

pub(crate) struct Upload {
    id: String,
    path: PathBuf,
    size: u64,
    received: u64,
    file: tokio::fs::File,
}

impl Upload {
    /// Closes and removes the partial file.
    fn discard(self) {
        drop(self.file);
        if let Err(e) = fs::remove_file(&self.path) {
            tracing::warn!(code = "backup.cleanup_failed", file = %self.path.display(), error = %e, "left behind");
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct UploadStart {
    /// The backup file's size in bytes.
    size: u64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub(crate) struct UploadStarted {
    id: String,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct PartQuery {
    /// Where this part starts: the bytes received so far.
    offset: u64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub(crate) struct UploadProgress {
    /// Bytes received so far.
    received: u64,
}

fn gone() -> Response {
    error(StatusCode::NOT_FOUND, "backup.upload_gone")
}

/// Starts a restore upload of a file of `size` bytes, replacing any earlier upload. Refused before anything is written
/// when the file is above the cap or the disk has less than twice its size free (the upload, then its unpacked copy).
#[utoipa::path(
    post,
    path = "/api/admin/backup/restore/upload",
    tag = "admin",
    request_body = UploadStart,
    responses(
        (status = 200, body = UploadStarted),
        (status = 400, description = "`backup.invalid`: empty", body = ErrorBody),
        (status = 413, description = "`backup.too_large`", body = ErrorBody),
        (status = 507, description = "`backup.no_space`, with `needed` (bytes)", body = ErrorBody),
        (status = 500, description = "`backup.restore_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn start(
    State(s): State<Arc<BackupState>>,
    Json(req): Json<UploadStart>,
) -> Response {
    if req.size == 0 {
        return BackupError::Invalid.response();
    }
    if req.size > MAX_ARCHIVE_BYTES {
        return BackupError::TooLarge.response();
    }
    let cache = s.data_dir.join("cache");
    if let Err(e) = fs::create_dir_all(&cache) {
        return stage_failed(e).response();
    }
    let mut slot = s.upload.lock().await;
    if let Some(old) = slot.take() {
        old.discard();
    }
    if let Err(e) = room(req.size.saturating_mul(2), (s.free_space)(&cache)) {
        return e.response();
    }
    let id = uuid::Uuid::now_v7().to_string();
    let path = cache.join(format!("{PREFIX}{id}.part"));
    let file = match tokio::fs::File::create(&path).await {
        Ok(file) => file,
        Err(e) => return stage_failed(e).response(),
    };
    *slot = Some(Upload {
        id: id.clone(),
        path,
        size: req.size,
        received: 0,
        file,
    });
    Json(UploadStarted { id }).into_response()
}

/// Adds the next part (the body, at most 16 MB) at `offset`, which must be the bytes received so far.
#[utoipa::path(
    put,
    path = "/api/admin/backup/restore/upload/{id}",
    tag = "admin",
    params(("id" = String, Path, description = "From the upload's start"), PartQuery),
    request_body(content = String, content_type = "application/octet-stream", description = "The next part"),
    responses(
        (status = 200, body = UploadProgress),
        (status = 400, description = "`backup.invalid`: past the size given at the start", body = ErrorBody),
        (status = 404, description = "`backup.upload_gone`: cancelled, replaced or never started", body = ErrorBody),
        (status = 409, description = "`backup.upload_offset`, with `received`: not where the upload stands", body = ErrorBody),
        (status = 413, description = "`backup.too_large`: a part above 16 MB", body = ErrorBody),
        (status = 500, description = "`backup.restore_failed`: could not be written; the upload is dropped", body = ErrorBody),
    )
)]
pub(crate) async fn part(
    State(s): State<Arc<BackupState>>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<PartQuery>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let bytes = match body {
        Ok(bytes) => bytes,
        Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return BackupError::TooLarge.response();
        }
        Err(_) => return BackupError::Invalid.response(),
    };
    let mut slot = s.upload.lock().await;
    let Some(upload) = slot.as_mut().filter(|u| u.id == id) else {
        return gone();
    };
    if q.offset != upload.received {
        return (
            StatusCode::CONFLICT,
            Json(
                serde_json::json!({ "code": "backup.upload_offset", "received": upload.received }),
            ),
        )
            .into_response();
    }
    if upload.received + bytes.len() as u64 > upload.size {
        return BackupError::Invalid.response();
    }
    if let Err(e) = upload.file.write_all(&bytes).await {
        // Never half a file kept for later.
        if let Some(upload) = slot.take() {
            upload.discard();
        }
        return stage_failed(e).response();
    }
    upload.received += bytes.len() as u64;
    Json(UploadProgress {
        received: upload.received,
    })
    .into_response()
}

/// Checks the complete upload and stages it for a restore (as the one-request preview did); nothing in use changes.
/// The uploaded file is removed either way.
#[utoipa::path(
    post,
    path = "/api/admin/backup/restore/upload/{id}/check",
    tag = "admin",
    params(("id" = String, Path, description = "From the upload's start")),
    responses(
        (status = 200, body = RestorePreview),
        (status = 400, description = "`backup.invalid`: damaged or not a backup; `backup.newer_version`: made by a newer version", body = ErrorBody),
        (status = 404, description = "`backup.upload_gone`", body = ErrorBody),
        (status = 409, description = "`backup.upload_offset`, with `received`: not all of it has arrived", body = ErrorBody),
        (status = 413, description = "`backup.too_large`", body = ErrorBody),
        (status = 500, description = "`backup.restore_failed`: could not be staged", body = ErrorBody),
    )
)]
pub(crate) async fn check(
    State(s): State<Arc<BackupState>>,
    UrlPath(id): UrlPath<String>,
) -> Response {
    let mut upload = {
        let mut slot = s.upload.lock().await;
        match slot.as_ref().filter(|u| u.id == id) {
            None => return gone(),
            Some(u) if u.received != u.size => {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({ "code": "backup.upload_offset", "received": u.received })),
                )
                    .into_response();
            }
            Some(_) => slot.take().expect("checked above"),
        }
    };
    let flushed = upload.file.flush().await;
    let path = upload.path.clone();
    let result = match flushed {
        Err(e) => Err(stage_failed(e)),
        Ok(()) => {
            let _one_at_a_time = s.lock.lock().await;
            stage(&s.data_dir, &path).await
        }
    };
    upload.discard();
    match result {
        Ok(preview) => Json(preview).into_response(),
        Err(e) => e.response(),
    }
}

/// Cancels the upload and removes what arrived; one already gone is fine.
#[utoipa::path(
    delete,
    path = "/api/admin/backup/restore/upload/{id}",
    tag = "admin",
    params(("id" = String, Path, description = "From the upload's start")),
    responses((status = 204, description = "Cancelled"))
)]
pub(crate) async fn cancel(
    State(s): State<Arc<BackupState>>,
    UrlPath(id): UrlPath<String>,
) -> StatusCode {
    let mut slot = s.upload.lock().await;
    if slot.as_ref().is_some_and(|u| u.id == id)
        && let Some(upload) = slot.take()
    {
        upload.discard();
    }
    StatusCode::NO_CONTENT
}

/// Removes every upload file left in `cache/` (cancelled, cut off, or the app ended mid-way). Call it on start.
pub(super) fn clean(data_dir: &Path) {
    let Ok(entries) = fs::read_dir(data_dir.join("cache")) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(PREFIX)
            && let Err(e) = fs::remove_file(entry.path())
        {
            tracing::warn!(code = "backup.cleanup_failed", file = %entry.path().display(), error = %e, "left behind");
        }
    }
}
