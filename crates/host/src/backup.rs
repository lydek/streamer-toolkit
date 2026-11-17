//! Backup files (`.stbackup`, docs/architecture.md §6 資料夾): a zip of `settings.json`, a `VACUUM INTO` snapshot
//! of the database, `templates/` and `sounds/`, plus `manifest.json` with every file's size and SHA-256.
//! Credentials live in the Windows Credential Manager, never in the data folder, so no backup holds them.
//! A restore is checked and staged by the admin API, then applied on the next start before the database opens.
//! Admin routes; the caller adds the token check.

use std::{
    collections::HashSet,
    fs,
    io::{self, Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State, rejection::BytesRejection},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    admin::{ErrorBody, error},
    db::{self, SqlitePool},
};

const FORMAT: &str = "streamer-toolkit-backup";
const FORMAT_VERSION: u32 = 1;
const MANIFEST: &str = "manifest.json";
const SETTINGS: &str = "settings.json";
/// Data folder subfolders taken whole. Never `logs/`, `cache/`, `WebView2/`, `backups/` or `instance-id`.
const FOLDERS: [&str; 2] = ["templates", "sounds"];
/// What a restore accepts; an export beyond these is refused, so every backup restores.
const MAX_ARCHIVE_BYTES: usize = 512 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
/// A checked backup waiting for the restart, in the data folder; its `manifest.json` is written last, once complete.
const STAGING_DIR: &str = "restore-staging";
/// Written when the broadcaster confirms; the next start applies the staged backup.
const PENDING: &str = "restore-pending";
/// The data a restore replaced, kept until the restore has finished so a failure can put it back.
const PREVIOUS_DIR: &str = "restore-previous";
/// In `restore-previous/` once every current item is in it; from then on, items without a copy there came from
/// the backup.
const MOVED: &str = ".moved";
/// What a restore replaces. The database's WAL files go too: SQLite would apply a stale WAL to the restored file.
const REPLACED: [&str; 6] = [
    SETTINGS,
    db::DB_FILE,
    "toolkit.db-wal",
    "toolkit.db-shm",
    "templates",
    "sounds",
];
/// What a backup puts in place.
const RESTORED: [&str; 4] = [SETTINGS, db::DB_FILE, "templates", "sounds"];

pub(crate) struct BackupState {
    pub db: SqlitePool,
    pub data_dir: PathBuf,
    pub app_version: String,
    /// Restarts the app so the next start applies a confirmed restore.
    pub restart: Arc<dyn Fn() + Send + Sync>,
    /// One staging or confirmation at a time.
    pub lock: tokio::sync::Mutex<()>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    format: String,
    format_version: u32,
    app_version: String,
    /// The newest migration applied to the database; 0 for an empty one.
    schema_version: i64,
    /// RFC 3339, UTC.
    created_at: String,
    records: Records,
    files: Vec<FileEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Records {
    redemptions: i64,
    checkins: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct FileEntry {
    path: String,
    size: u64,
    sha256: String,
}

/// What a backup holds, shown before the broadcaster confirms a restore.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct RestorePreview {
    /// When the backup was made (RFC 3339, UTC).
    pub created_at: String,
    /// The version of the app that made it.
    pub app_version: String,
    pub redemptions: i64,
    pub checkins: i64,
    /// Template folders, e.g. `templates/chat/`.
    pub templates: usize,
    pub sounds: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("backup.too_large")]
    TooLarge,
    #[error("backup.export_failed: {0}")]
    Export(String),
    /// Damaged, tampered with, or not a backup at all.
    #[error("backup.invalid")]
    Invalid,
    /// Made by a newer version of the app.
    #[error("backup.newer_version")]
    NewerVersion,
    #[error("backup.stage_failed: {0}")]
    Stage(String),
    #[error("backup.nothing_staged")]
    NothingStaged,
    /// The restore failed; the data from before it is back in place.
    #[error("backup.restore_failed: {0}")]
    Restore(String),
    /// A restore was cut short (the app ended mid-way); the data from before it is back in place.
    #[error("backup.restore_interrupted")]
    Interrupted,
    /// The restore failed and the data from before it could not all be put back: it stays in `restore-previous/`
    /// and the next start tries again.
    #[error("backup.put_back_failed: {0}")]
    PutBack(String),
}

impl BackupError {
    fn response(&self) -> Response {
        let (status, code) = match self {
            Self::TooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "backup.too_large"),
            Self::Invalid => (StatusCode::BAD_REQUEST, "backup.invalid"),
            Self::NewerVersion => (StatusCode::BAD_REQUEST, "backup.newer_version"),
            Self::NothingStaged => (StatusCode::CONFLICT, "backup.nothing_staged"),
            Self::Export(_) => (StatusCode::INTERNAL_SERVER_ERROR, "backup.export_failed"),
            Self::Stage(_) | Self::Restore(_) | Self::Interrupted | Self::PutBack(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "backup.restore_failed")
            }
        };
        if status.is_server_error() {
            tracing::error!(code, error = %self, "backup or restore failed");
        }
        error(status, code)
    }
}

fn export_failed(e: impl std::fmt::Display) -> BackupError {
    BackupError::Export(e.to_string())
}

fn stage_failed(e: impl std::fmt::Display) -> BackupError {
    BackupError::Stage(e.to_string())
}

fn restore_failed(e: impl std::fmt::Display) -> BackupError {
    BackupError::Restore(e.to_string())
}

pub(crate) fn routes(state: BackupState) -> Router {
    Router::new()
        .route("/api/admin/backup", get(export_backup))
        .route(
            "/api/admin/backup/restore/preview",
            post(preview_restore).layer(DefaultBodyLimit::max(MAX_ARCHIVE_BYTES)),
        )
        .route("/api/admin/backup/restore", post(confirm_restore))
        .with_state(Arc::new(state))
}

/// A `.stbackup` file of this data folder: settings, the database, custom templates and sounds. Live use goes
/// on while it is made; database writes wait only for the snapshot.
#[utoipa::path(
    get,
    path = "/api/admin/backup",
    tag = "admin",
    responses(
        (status = 200, description = "The backup file", content_type = "application/octet-stream", body = String),
        (status = 413, description = "`backup.too_large`: more than a restore accepts", body = ErrorBody),
        (status = 500, description = "`backup.export_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn export_backup(State(s): State<Arc<BackupState>>) -> Response {
    match export(&s.db, &s.data_dir, &s.app_version).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "application/octet-stream"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            bytes,
        )
            .into_response(),
        Err(e) => e.response(),
    }
}

/// Checks a backup file (at most 512 MB) and stages it for a restore; nothing in use changes. The body is the
/// file itself. A later preview replaces the staged one.
#[utoipa::path(
    post,
    path = "/api/admin/backup/restore/preview",
    tag = "admin",
    request_body(content = String, content_type = "application/octet-stream", description = "The backup file"),
    responses(
        (status = 200, body = RestorePreview),
        (status = 400, description = "`backup.invalid`: damaged or not a backup; `backup.newer_version`: made by a newer version", body = ErrorBody),
        (status = 413, description = "`backup.too_large`", body = ErrorBody),
        (status = 500, description = "`backup.restore_failed`: could not be staged", body = ErrorBody),
    )
)]
pub(crate) async fn preview_restore(
    State(s): State<Arc<BackupState>>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let bytes = match body {
        Ok(bytes) => bytes,
        Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return BackupError::TooLarge.response();
        }
        Err(_) => return BackupError::Invalid.response(),
    };
    let _one_at_a_time = s.lock.lock().await;
    match stage(&s.data_dir, bytes).await {
        Ok(preview) => Json(preview).into_response(),
        Err(e) => e.response(),
    }
}

/// Restores the staged backup: the app restarts, backs up the current data to `backups/`, then puts the backup
/// in place before the database opens.
#[utoipa::path(
    post,
    path = "/api/admin/backup/restore",
    tag = "admin",
    responses(
        (status = 202, description = "The app restarts to restore"),
        (status = 409, description = "`backup.nothing_staged`", body = ErrorBody),
        (status = 500, description = "`backup.restore_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn confirm_restore(State(s): State<Arc<BackupState>>) -> Response {
    let _one_at_a_time = s.lock.lock().await;
    if !s.data_dir.join(STAGING_DIR).join(MANIFEST).is_file() {
        return BackupError::NothingStaged.response();
    }
    if let Err(e) = fs::write(s.data_dir.join(PENDING), b"") {
        return restore_failed(e).response();
    }
    let restart = s.restart.clone();
    tokio::spawn(async move {
        // Lets the response reach the admin window first.
        tokio::time::sleep(Duration::from_millis(500)).await;
        restart();
    });
    StatusCode::ACCEPTED.into_response()
}

/// The backup file's bytes. The snapshot goes to `cache/` and is removed afterwards.
async fn export(
    pool: &SqlitePool,
    data_dir: &Path,
    app_version: &str,
) -> Result<Vec<u8>, BackupError> {
    let cache = data_dir.join("cache");
    std::fs::create_dir_all(&cache).map_err(export_failed)?;
    let snapshot = cache.join(format!("backup-{}.db", uuid::Uuid::now_v7()));
    let result = export_snapshot(pool, data_dir, app_version, &snapshot).await;
    let _ = std::fs::remove_file(&snapshot);
    result
}

async fn export_snapshot(
    pool: &SqlitePool,
    data_dir: &Path,
    app_version: &str,
    snapshot: &Path,
) -> Result<Vec<u8>, BackupError> {
    db::snapshot(pool, snapshot).await.map_err(export_failed)?;
    let (schema_version, records) = describe(snapshot).await.map_err(export_failed)?;
    let manifest = Manifest {
        format: FORMAT.into(),
        format_version: FORMAT_VERSION,
        app_version: app_version.into(),
        schema_version,
        created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        records,
        files: Vec::new(),
    };
    let (data_dir, snapshot) = (data_dir.to_owned(), snapshot.to_owned());
    tokio::task::spawn_blocking(move || write_archive(&data_dir, &snapshot, manifest))
        .await
        .map_err(export_failed)?
}

/// The snapshot's schema version and record counts, read through the modules' public counts.
async fn describe(snapshot: &Path) -> Result<(i64, Records), sqlx::Error> {
    let pool = db::open_file(snapshot).await?;
    let result = async {
        let version = db::schema_version(&pool).await?;
        // An empty database has no module tables yet.
        let records = if version == 0 {
            Records {
                redemptions: 0,
                checkins: 0,
            }
        } else {
            Records {
                redemptions: redemption::record_count(&pool).await?,
                checkins: checkin::record_count(&pool).await?,
            }
        };
        Ok((version, records))
    }
    .await;
    pool.close().await;
    result
}

fn write_archive(
    data_dir: &Path,
    snapshot: &Path,
    mut manifest: Manifest,
) -> Result<Vec<u8>, BackupError> {
    let mut files = Vec::new();
    let settings = data_dir.join(SETTINGS);
    if settings.is_file() {
        files.push((SETTINGS.to_owned(), settings));
    }
    files.push((db::DB_FILE.to_owned(), snapshot.to_owned()));
    for folder in FOLDERS {
        collect(&data_dir.join(folder), folder, &mut files).map_err(export_failed)?;
    }
    if files.len() >= MAX_ENTRIES {
        return Err(BackupError::TooLarge);
    }
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let mut expanded = 0;
    for (name, path) in files {
        let bytes = std::fs::read(&path).map_err(export_failed)?;
        let size = bytes.len() as u64;
        expanded += size;
        if expanded > MAX_EXPANDED_BYTES {
            return Err(BackupError::TooLarge);
        }
        zip.start_file(name.as_str(), options)
            .map_err(export_failed)?;
        zip.write_all(&bytes).map_err(export_failed)?;
        manifest.files.push(FileEntry {
            path: name,
            size,
            sha256: sha256(&bytes),
        });
    }
    zip.start_file(MANIFEST, options).map_err(export_failed)?;
    let text = serde_json::to_vec_pretty(&manifest).map_err(export_failed)?;
    zip.write_all(&text).map_err(export_failed)?;
    let bytes = zip.finish().map_err(export_failed)?.into_inner();
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(BackupError::TooLarge);
    }
    Ok(bytes)
}

/// Every regular file under `dir`, named `<prefix>/<relative path>`. Symbolic links and junctions are skipped: they
/// may point outside the data folder.
fn collect(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) -> io::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        // Does not follow links.
        let kind = entry.file_type()?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|n| io::Error::other(format!("file name is not Unicode: {n:?}")))?;
        let path = format!("{prefix}/{name}");
        if kind.is_dir() {
            collect(&entry.path(), &path, out)?;
        } else if kind.is_file() {
            // A name a restore would refuse would make a backup that never restores.
            if !allowed_path(&path) {
                return Err(io::Error::other(format!("unsupported file name: {path}")));
            }
            out.push((path, entry.path()));
        }
    }
    Ok(())
}

/// `settings.json`, `toolkit.db`, or a file under `templates/` or `sounds/`, as plain relative segments that are
/// safe to create on Windows.
fn allowed_path(path: &str) -> bool {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let segments: Vec<&str> = path.split('/').collect();
    let plain = |s: &&str| {
        let stem = s.split('.').next().unwrap_or_default();
        !s.is_empty()
            && s.len() <= 255
            && *s != "."
            && *s != ".."
            && !s.ends_with(['.', ' '])
            && !s.contains(['\\', ':', '*', '?', '"', '<', '>', '|'])
            && !s.chars().any(char::is_control)
            && !RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r))
    };
    path.len() <= 1024
        && segments.iter().all(plain)
        && match segments.as_slice() {
            [single] => *single == SETTINGS || *single == db::DB_FILE,
            [folder, _, ..] => FOLDERS.contains(folder),
            [] => false,
        }
}

fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Reads `entry` through SHA-256 into `out`, at most one byte more than the manifest says; the size and hash must
/// match it. The zip's own size fields are never trusted.
fn copy_checked(
    entry: impl Read,
    file: &FileEntry,
    out: &mut impl Write,
) -> Result<(), BackupError> {
    let mut entry = entry.take(file.size + 1);
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 64 * 1024];
    let mut size = 0;
    loop {
        // Damaged data (deflate or CRC) fails here.
        let n = entry.read(&mut buf).map_err(|_| BackupError::Invalid)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(stage_failed)?;
    }
    if size != file.size || hex(&hasher.finalize()) != file.sha256 {
        return Err(BackupError::Invalid);
    }
    Ok(())
}

type Archive<'a> = ZipArchive<Cursor<&'a [u8]>>;

/// Checks a backup in memory before anything is written: the limits, every path, each file's size and hash, and
/// that this version can read it.
fn validate(bytes: &[u8]) -> Result<(Manifest, Archive<'_>), BackupError> {
    use BackupError::{Invalid, NewerVersion, TooLarge};
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(TooLarge);
    }
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|_| Invalid)?;
    if zip.len() > MAX_ENTRIES {
        return Err(TooLarge);
    }
    let mut text = Vec::new();
    zip.by_name(MANIFEST)
        .map_err(|_| Invalid)?
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut text)
        .map_err(|_| Invalid)?;
    if text.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(Invalid);
    }
    let manifest: Manifest = serde_json::from_slice(&text).map_err(|_| Invalid)?;
    if manifest.format != FORMAT || manifest.format_version == 0 {
        return Err(Invalid);
    }
    if manifest.format_version > FORMAT_VERSION || manifest.schema_version > db::latest_version() {
        return Err(NewerVersion);
    }
    let mut paths = HashSet::new();
    let mut expanded = 0u64;
    for file in &manifest.files {
        if !allowed_path(&file.path) || !paths.insert(file.path.as_str()) {
            return Err(Invalid);
        }
        expanded = expanded.saturating_add(file.size);
    }
    if expanded > MAX_EXPANDED_BYTES {
        return Err(TooLarge);
    }
    // Exactly the listed files plus the manifest: nothing unlisted, nothing twice.
    if !paths.contains(db::DB_FILE) || zip.len() != manifest.files.len() + 1 {
        return Err(Invalid);
    }
    for file in &manifest.files {
        let entry = zip.by_name(&file.path).map_err(|_| Invalid)?;
        copy_checked(entry, file, &mut io::sink())?;
    }
    Ok((manifest, zip))
}

/// Checks the backup and writes it to `restore-staging/`, replacing anything staged before; on any failure nothing
/// stays staged. The data in use is never touched.
async fn stage(data_dir: &Path, bytes: Bytes) -> Result<RestorePreview, BackupError> {
    let staging = data_dir.join(STAGING_DIR);
    let result = stage_into(&staging, bytes).await;
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

async fn stage_into(staging: &Path, bytes: Bytes) -> Result<RestorePreview, BackupError> {
    let dir = staging.to_owned();
    let manifest = tokio::task::spawn_blocking(move || extract(&bytes, &dir))
        .await
        .map_err(stage_failed)??;
    // The database itself, not only the manifest's claim: every applied migration must be one this build knows.
    match db::fit(&staging.join(db::DB_FILE))
        .await
        .map_err(|_| BackupError::Invalid)?
    {
        db::Fit::Known => {}
        db::Fit::Newer => return Err(BackupError::NewerVersion),
        db::Fit::Changed => return Err(BackupError::Invalid),
    }
    // Last: only a complete staging has a manifest.
    let text = serde_json::to_vec_pretty(&manifest).map_err(stage_failed)?;
    fs::write(staging.join(MANIFEST), text).map_err(stage_failed)?;
    Ok(preview(&manifest))
}

fn extract(bytes: &[u8], staging: &Path) -> Result<Manifest, BackupError> {
    let (manifest, mut zip) = validate(bytes)?;
    remove(staging).map_err(stage_failed)?;
    for file in &manifest.files {
        // Checked by `allowed_path`: plain relative segments, so the target stays inside the staging folder.
        let target = staging.join(&file.path);
        fs::create_dir_all(target.parent().unwrap_or(staging)).map_err(stage_failed)?;
        let mut out = fs::File::create(&target).map_err(stage_failed)?;
        let entry = zip.by_name(&file.path).map_err(|_| BackupError::Invalid)?;
        copy_checked(entry, file, &mut out)?;
    }
    // Unreadable settings would silently fall back to defaults after the restart.
    if let Ok(text) = fs::read(staging.join(SETTINGS)) {
        serde_json::from_slice::<toolkit_core::settings::Settings>(&text)
            .map_err(|_| BackupError::Invalid)?;
    }
    Ok(manifest)
}

fn preview(m: &Manifest) -> RestorePreview {
    let under = |folder: &'static str| {
        m.files
            .iter()
            .filter_map(move |f| f.path.strip_prefix(folder)?.strip_prefix('/'))
    };
    RestorePreview {
        created_at: m.created_at.clone(),
        app_version: m.app_version.clone(),
        redemptions: m.records.redemptions,
        checkins: m.records.checkins,
        templates: under("templates")
            .filter_map(|rest| Some(rest.split_once('/')?.0))
            .collect::<HashSet<_>>()
            .len(),
        sounds: under("sounds").count(),
    }
}

/// Applies a restore confirmed in the admin UI. Call it on start, after the single-instance lock and before the
/// database opens (docs/architecture.md §6). Returns whether a restore was applied. On any failure the data from
/// before is back in place, except for [`BackupError::PutBack`].
pub async fn apply_pending(data_dir: &Path, app_version: &str) -> Result<bool, BackupError> {
    let pending = data_dir.join(PENDING);
    let staging = data_dir.join(STAGING_DIR);
    let previous = data_dir.join(PREVIOUS_DIR);
    let result = if !pending.exists() {
        Ok(false)
    } else if previous.exists() {
        // The app ended in the middle of a restore.
        put_back(data_dir, &previous)
            .map_err(|e| BackupError::PutBack(e.to_string()))
            .and(Err(BackupError::Interrupted))
    } else {
        apply(data_dir, &staging, &previous, app_version)
            .await
            .map(|()| true)
    };
    if matches!(result, Err(BackupError::PutBack(_))) {
        // The marker and the replaced data stay: the next start tries again.
        return result;
    }
    let _ = fs::remove_file(&pending);
    for dir in [&previous, &staging] {
        if let Err(e) = remove(dir) {
            tracing::warn!(code = "backup.cleanup_failed", dir = %dir.display(), error = %e, "left behind");
        }
    }
    result
}

async fn apply(
    data_dir: &Path,
    staging: &Path,
    previous: &Path,
    app_version: &str,
) -> Result<(), BackupError> {
    if !staging.join(MANIFEST).is_file() {
        return Err(BackupError::NothingStaged);
    }
    back_up_current(data_dir, app_version).await?;
    let result = async {
        swap(data_dir, staging, previous).map_err(restore_failed)?;
        // The normal start path: pending migrations run, after their own backup.
        db::open(data_dir)
            .await
            .map_err(restore_failed)?
            .close()
            .await;
        Ok(())
    }
    .await;
    if let Err(e) = result {
        put_back(data_dir, previous).map_err(|p| BackupError::PutBack(format!("{e}; {p}")))?;
        return Err(e);
    }
    Ok(())
}

/// Saves the current data as a `.stbackup` in `backups/`, so the admin UI can restore it again.
async fn back_up_current(data_dir: &Path, app_version: &str) -> Result<PathBuf, BackupError> {
    let db_file = data_dir.join(db::DB_FILE);
    if !db_file.exists() {
        // An empty file is an empty database.
        fs::write(&db_file, b"").map_err(restore_failed)?;
    }
    let pool = db::open_file(&db_file).await.map_err(restore_failed)?;
    let bytes = export(&pool, data_dir, app_version).await;
    pool.close().await;
    let bytes = bytes.map_err(|e| restore_failed(format!("backing up the current data: {e}")))?;
    let dir = data_dir.join(db::BACKUP_DIR);
    fs::create_dir_all(&dir).map_err(restore_failed)?;
    let path = dir.join(format!("before-restore-{}.stbackup", db::stamp()));
    fs::write(&path, bytes).map_err(restore_failed)?;
    tracing::info!(backup = %path.display(), "current data backed up before restoring");
    Ok(path)
}

/// Moves the current items into `restore-previous/`, then the staged ones into place; renames only, all inside the
/// data folder.
fn swap(data_dir: &Path, staging: &Path, previous: &Path) -> io::Result<()> {
    fs::create_dir_all(previous)?;
    for item in REPLACED {
        let current = data_dir.join(item);
        if fs::symlink_metadata(&current).is_ok() {
            fs::rename(&current, previous.join(item))?;
        }
    }
    fs::write(previous.join(MOVED), b"")?;
    for item in RESTORED {
        let staged = staging.join(item);
        if staged.exists() {
            fs::rename(&staged, data_dir.join(item))?;
        }
    }
    Ok(())
}

/// Undoes [`swap`], also one cut short: what came from the backup goes, the replaced items come back.
fn put_back(data_dir: &Path, previous: &Path) -> io::Result<()> {
    let all_moved = previous.join(MOVED).exists();
    for item in REPLACED {
        let (current, kept) = (data_dir.join(item), previous.join(item));
        let has_kept = fs::symlink_metadata(&kept).is_ok();
        // Before every item was moved, one without a copy is still the original.
        if has_kept || all_moved {
            remove(&current)?;
        }
        if has_kept {
            fs::rename(&kept, &current)?;
        }
    }
    Ok(())
}

/// Removes a file or a folder; a missing one is fine.
fn remove(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    pub(super) fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-backup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A data folder with something in every place a backup takes or must leave out.
    pub(super) async fn sample_folder(name: &str) -> (PathBuf, SqlitePool) {
        let dir = temp_dir(name);
        toolkit_core::settings::update(&dir, |s| {
            s.twitch_client_id = "clientid123".into();
            s.chat_template = "custom".into();
        })
        .unwrap();
        let write = |path: &str, text: &str| {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write("templates/chat/template.html", "<div data-chat-list></div>");
        write(
            "sounds/0190a1b2-0000-7000-8000-000000000001.mp3",
            "ID3 sound",
        );
        write("logs/streamer-toolkit.2026-11-17.log", "log line");
        write("cache/thing", "cached");
        write("WebView2/Default/Cookies", "cookies");
        write("instance-id", "0123456789abcdef0123456789abcdef");
        write("backups/toolkit-old.db", "old backup");
        let pool = db::open(&dir).await.unwrap();
        for (id, name) in [("r1", "觀眾一"), ("r2", "viewer2")] {
            sqlx::query(
                "INSERT INTO redemption_records (id, twitch_redemption_id, user_id, user_login, user_name, \
                 reward_id, reward_title, reward_cost, user_input, status, redeemed_at, updated_at) \
                 VALUES (?, ?, 'u', 'login', ?, 'reward', 'title', 100, '', 'unfulfilled', \
                 '2026-11-17T00:00:00Z', '2026-11-17T00:00:00Z')",
            )
            .bind(id)
            .bind(format!("twitch-{id}"))
            .bind(name)
            .execute(&pool)
            .await
            .unwrap();
        }
        (dir, pool)
    }

    fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        (0..zip.len())
            .map(|i| {
                let mut file = zip.by_index(i).unwrap();
                let mut content = Vec::new();
                file.read_to_end(&mut content).unwrap();
                (file.name().unwrap().into_owned(), content)
            })
            .collect()
    }

    #[tokio::test]
    async fn export_takes_settings_database_templates_and_sounds_only() {
        let (dir, pool) = sample_folder("export").await;
        let bytes = export(&pool, &dir, "1.2.3").await.unwrap();
        let entries = entries(&bytes);
        let mut names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        names.sort();
        assert_eq!(
            names,
            [
                MANIFEST,
                SETTINGS,
                "sounds/0190a1b2-0000-7000-8000-000000000001.mp3",
                "templates/chat/template.html",
                db::DB_FILE,
            ]
        );

        let (_, text) = entries.iter().find(|(n, _)| n == MANIFEST).unwrap();
        let manifest: Manifest = serde_json::from_slice(text).unwrap();
        assert_eq!(
            (manifest.format.as_str(), manifest.format_version),
            (FORMAT, 1)
        );
        assert_eq!(manifest.app_version, "1.2.3");
        assert!(manifest.schema_version > 0);
        assert_eq!(
            (manifest.records.redemptions, manifest.records.checkins),
            (2, 0)
        );
        assert_eq!(manifest.files.len(), 4);
        for file in &manifest.files {
            let (_, content) = entries.iter().find(|(n, _)| *n == file.path).unwrap();
            assert_eq!(file.size, content.len() as u64);
            assert_eq!(file.sha256, sha256(content));
        }
        // The snapshot is a database of its own and leaves nothing behind in cache/.
        let cache: Vec<_> = std::fs::read_dir(dir.join("cache")).unwrap().collect();
        assert_eq!(cache.len(), 1, "only the sample file");
        pool.close().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn export_holds_no_credentials() {
        let (dir, pool) = sample_folder("secrets").await;
        // What the Credential Manager holds for this folder (docs/architecture.md): never in the data folder.
        let secrets = [
            "access_token",
            "refresh_token",
            "0123456789abcdef0123456789abcdef",
        ];
        let bytes = export(&pool, &dir, "1.2.3").await.unwrap();
        for (name, content) in entries(&bytes) {
            let text = String::from_utf8_lossy(&content).to_lowercase();
            for secret in secrets {
                assert!(!text.contains(secret), "{name} contains {secret}");
            }
            assert!(!text.contains("password"), "{name} mentions a password");
        }
        pool.close().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn only_plain_paths_in_known_places_are_allowed() {
        for ok in [
            "settings.json",
            "toolkit.db",
            "templates/chat/style.css",
            "sounds/a.mp3",
            "templates/chat/圖片/貓.png",
        ] {
            assert!(allowed_path(ok), "{ok}");
        }
        for bad in [
            "",
            "/settings.json",
            "C:/Windows/x",
            "../settings.json",
            "templates/../../x",
            "templates/./x",
            "templates//x",
            "templates",
            "sounds/",
            "templates\\x",
            "logs/a.log",
            "instance-id",
            "settings.json/x",
            "templates/CON",
            "templates/nul.txt",
            "templates/x.",
            "templates/x ",
            "templates/a:b",
            "templates/a\u{0}b",
        ] {
            assert!(!allowed_path(bad), "{bad:?}");
        }
    }

    /// What a restore must bring back: settings, templates and sounds, and database rows.
    #[derive(Debug, PartialEq)]
    struct FolderState {
        settings: Vec<u8>,
        files: Vec<(String, Vec<u8>)>,
        rows: Vec<(String, String)>,
    }

    async fn state_of(dir: &Path) -> FolderState {
        let mut paths = Vec::new();
        for folder in FOLDERS {
            collect(&dir.join(folder), folder, &mut paths).unwrap();
        }
        let mut files: Vec<_> = paths
            .into_iter()
            .map(|(name, path)| (name, fs::read(path).unwrap()))
            .collect();
        files.sort();
        let pool = db::open_file(&dir.join(db::DB_FILE)).await.unwrap();
        let rows = sqlx::query_as("SELECT id, user_name FROM redemption_records ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        pool.close().await;
        FolderState {
            settings: fs::read(dir.join(SETTINGS)).unwrap_or_default(),
            files,
            rows,
        }
    }

    fn leftovers(dir: &Path) -> Vec<&'static str> {
        [STAGING_DIR, PENDING, PREVIOUS_DIR]
            .into_iter()
            .filter(|l| dir.join(l).exists())
            .collect()
    }

    fn test_state(dir: &Path, pool: SqlitePool, restarted: Arc<AtomicBool>) -> Arc<BackupState> {
        Arc::new(BackupState {
            db: pool,
            data_dir: dir.to_owned(),
            app_version: "1.2.3".into(),
            restart: Arc::new(move || restarted.store(true, Ordering::SeqCst)),
            lock: Default::default(),
        })
    }

    fn repack(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, content) in entries {
            zip.start_file(name.as_str(), SimpleFileOptions::default())
                .unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    /// The backup with its manifest changed as JSON.
    fn with_manifest(bytes: &[u8], edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
        let mut entries = entries(bytes);
        let (_, text) = entries.iter_mut().find(|(n, _)| n == MANIFEST).unwrap();
        let mut manifest: serde_json::Value = serde_json::from_slice(text).unwrap();
        edit(&mut manifest);
        *text = serde_json::to_vec(&manifest).unwrap();
        repack(&entries)
    }

    /// The backup with one file renamed and replaced, its manifest entry matching: only other checks can catch it.
    fn with_file(bytes: &[u8], path: &str, new_path: &str, content: &[u8]) -> Vec<u8> {
        let mut entries = entries(bytes);
        let entry = entries.iter_mut().find(|(n, _)| n == path).unwrap();
        *entry = (new_path.to_owned(), content.to_vec());
        let renamed = repack(&entries);
        with_manifest(&renamed, |m| {
            let files = m["files"].as_array_mut().unwrap();
            let file = files.iter_mut().find(|f| f["path"] == path).unwrap();
            file["path"] = new_path.into();
            file["size"] = content.len().into();
            file["sha256"] = sha256(content).into();
        })
    }

    #[tokio::test]
    async fn export_then_restore_brings_back_the_same_settings_and_rows() {
        let (dir, pool) = sample_folder("round-trip").await;
        let exported = export(&pool, &dir, "1.2.3").await.unwrap();
        let before = state_of(&dir).await;
        // Everything changes after the export.
        toolkit_core::settings::update(&dir, |s| s.twitch_client_id = "changed".into()).unwrap();
        sqlx::query("DELETE FROM redemption_records WHERE id = 'r1'")
            .execute(&pool)
            .await
            .unwrap();
        fs::remove_dir_all(dir.join("templates")).unwrap();
        fs::write(dir.join("sounds/new.mp3"), "new").unwrap();
        let changed = state_of(&dir).await;
        assert_ne!(changed, before);

        let preview = stage(&dir, exported.into()).await.unwrap();
        assert_eq!(preview.app_version, "1.2.3");
        assert_eq!(
            (
                preview.redemptions,
                preview.checkins,
                preview.templates,
                preview.sounds
            ),
            (2, 0, 1, 1)
        );
        assert_eq!(
            state_of(&dir).await,
            changed,
            "staging changes nothing in use"
        );
        let restarted = Arc::new(AtomicBool::new(false));
        let state = test_state(&dir, pool.clone(), restarted.clone());
        assert_eq!(
            confirm_restore(State(state)).await.status(),
            StatusCode::ACCEPTED
        );
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert!(restarted.load(Ordering::SeqCst), "the app restarts");
        assert_eq!(
            state_of(&dir).await,
            changed,
            "nothing changes before the restart"
        );
        pool.close().await;

        // The next start.
        assert!(apply_pending(&dir, "1.2.4").await.unwrap());
        assert_eq!(state_of(&dir).await, before);
        assert_eq!(
            fs::read_to_string(dir.join("instance-id")).unwrap(),
            "0123456789abcdef0123456789abcdef",
            "credentials stay keyed by this folder"
        );
        assert!(dir.join("logs/streamer-toolkit.2026-11-17.log").exists());
        assert!(leftovers(&dir).is_empty(), "{:?}", leftovers(&dir));
        assert!(!apply_pending(&dir, "1.2.4").await.unwrap(), "applied once");

        // The data from before the restore was backed up to backups/, and restores too.
        let [saved] = fs::read_dir(dir.join(db::BACKUP_DIR))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.to_string_lossy().contains("before-restore-"))
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        assert!(saved.to_string_lossy().ends_with(".stbackup"));
        stage(&dir, fs::read(saved).unwrap().into()).await.unwrap();
        fs::write(dir.join(PENDING), b"").unwrap();
        assert!(apply_pending(&dir, "1.2.4").await.unwrap());
        assert_eq!(state_of(&dir).await, changed);
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn damaged_or_foreign_files_are_refused_and_nothing_changes() {
        let (dir, pool) = sample_folder("refused").await;
        let good = export(&pool, &dir, "1.2.3").await.unwrap();
        let before = state_of(&dir).await;
        let template = "templates/chat/template.html";
        let mut entries = entries(&good);
        let tampered = {
            let mut e = entries.clone();
            e.iter_mut().find(|(n, _)| n == template).unwrap().1 = b"<b>changed</b>".to_vec();
            repack(&e)
        };
        let unlisted = {
            let mut e = entries.clone();
            e.push(("sounds/unlisted.mp3".into(), b"x".to_vec()));
            repack(&e)
        };
        let no_manifest = repack(&entries[..entries.len() - 1]);
        entries.extend((0..MAX_ENTRIES).map(|i| (format!("sounds/{i}.mp3"), Vec::new())));
        let too_many = repack(&entries);
        let cases: Vec<(&str, Vec<u8>, &str)> = vec![
            ("not a zip", b"not a backup".to_vec(), "backup.invalid"),
            ("tampered", tampered, "backup.invalid"),
            ("unlisted file", unlisted, "backup.invalid"),
            ("no manifest", no_manifest, "backup.invalid"),
            ("too many files", too_many, "backup.too_large"),
            (
                "zip slip",
                with_file(&good, template, "templates/../../evil.html", b"x"),
                "backup.invalid",
            ),
            (
                "absolute path",
                with_file(&good, template, "C:/evil.html", b"x"),
                "backup.invalid",
            ),
            (
                "not a data folder file",
                with_file(&good, template, "instance-id", b"x"),
                "backup.invalid",
            ),
            (
                "unreadable settings",
                with_file(&good, SETTINGS, SETTINGS, br#"{"port":"x"}"#),
                "backup.invalid",
            ),
            (
                "another format",
                with_manifest(&good, |m| m["format"] = "zip".into()),
                "backup.invalid",
            ),
            (
                "expands too far",
                with_manifest(&good, |m| m["files"][0]["size"] = (3u64 << 30).into()),
                "backup.too_large",
            ),
            (
                "newer format",
                with_manifest(&good, |m| m["format_version"] = 2.into()),
                "backup.newer_version",
            ),
            (
                "newer schema",
                with_manifest(&good, |m| {
                    m["schema_version"] = (db::latest_version() + 1).into()
                }),
                "backup.newer_version",
            ),
        ];
        for (label, bytes, code) in cases {
            let e = stage(&dir, bytes.into())
                .await
                .err()
                .unwrap_or_else(|| panic!("{label} accepted"));
            assert_eq!(e.to_string(), code, "{label}");
            assert!(leftovers(&dir).is_empty(), "{label}: {:?}", leftovers(&dir));
            assert_eq!(state_of(&dir).await, before, "{label}");
        }
        // Nothing staged: there is nothing to confirm.
        let state = test_state(&dir, pool.clone(), Arc::default());
        assert_eq!(
            confirm_restore(State(state)).await.status(),
            StatusCode::CONFLICT
        );
        pool.close().await;
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_database_from_a_newer_version_is_refused_even_if_the_manifest_hides_it() {
        let (dir, pool) = sample_folder("newer").await;
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
             VALUES (?, 'future', 1, x'00', 0)",
        )
        .bind(db::latest_version() + 1)
        .execute(&pool)
        .await
        .unwrap();
        let newer = export(&pool, &dir, "9.0.0").await.unwrap();
        let before = state_of(&dir).await;
        let e = stage(&dir, newer.clone().into()).await.unwrap_err();
        assert_eq!(e.to_string(), "backup.newer_version");
        let hidden = with_manifest(&newer, |m| m["schema_version"] = 1.into());
        let e = stage(&dir, hidden.into()).await.unwrap_err();
        assert_eq!(e.to_string(), "backup.newer_version");
        assert!(leftovers(&dir).is_empty());
        assert_eq!(state_of(&dir).await, before);
        pool.close().await;
        let _ = fs::remove_dir_all(dir);
    }

    // An open file keeps Windows from moving its folder, which makes the swap fail half-way.
    #[cfg(windows)]
    #[tokio::test]
    async fn a_restore_that_fails_half_way_puts_the_data_back() {
        let (dir, pool) = sample_folder("half-way").await;
        let bytes = export(&pool, &dir, "1.2.3").await.unwrap();
        sqlx::query("DELETE FROM redemption_records")
            .execute(&pool)
            .await
            .unwrap();
        fs::write(dir.join("templates/chat/template.html"), "changed").unwrap();
        stage(&dir, bytes.into()).await.unwrap();
        fs::write(dir.join(PENDING), b"").unwrap();
        pool.close().await;
        let before = state_of(&dir).await;

        let held =
            fs::File::open(dir.join("sounds/0190a1b2-0000-7000-8000-000000000001.mp3")).unwrap();
        let e = apply_pending(&dir, "1.2.3").await.unwrap_err();
        drop(held);
        assert!(matches!(e, BackupError::Restore(_)), "{e}");
        assert_eq!(state_of(&dir).await, before);
        assert!(leftovers(&dir).is_empty(), "{:?}", leftovers(&dir));
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_restore_cut_short_is_undone_on_the_next_start() {
        let dir = temp_dir("interrupted");
        // As a swap leaves the folder when the app ends after moving everything out and part of the backup in.
        let previous = dir.join(PREVIOUS_DIR);
        fs::create_dir_all(previous.join("templates")).unwrap();
        fs::write(previous.join(SETTINGS), "old settings").unwrap();
        fs::write(previous.join(db::DB_FILE), "old db").unwrap();
        fs::write(previous.join("templates/old.css"), "old").unwrap();
        fs::write(previous.join(MOVED), "").unwrap();
        fs::write(dir.join(db::DB_FILE), "restored db").unwrap();
        fs::create_dir_all(dir.join("sounds")).unwrap();
        fs::write(dir.join("sounds/restored.mp3"), "restored").unwrap();
        fs::create_dir_all(dir.join(STAGING_DIR)).unwrap();
        fs::write(dir.join(PENDING), "").unwrap();

        let e = apply_pending(&dir, "1.2.3").await.unwrap_err();
        assert!(matches!(e, BackupError::Interrupted), "{e}");
        assert_eq!(
            fs::read_to_string(dir.join(SETTINGS)).unwrap(),
            "old settings"
        );
        assert_eq!(fs::read_to_string(dir.join(db::DB_FILE)).unwrap(), "old db");
        assert!(dir.join("templates/old.css").exists());
        assert!(!dir.join("sounds").exists(), "came from the backup");
        assert!(leftovers(&dir).is_empty(), "{:?}", leftovers(&dir));
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_staged_backup_never_confirmed_is_dropped_on_start() {
        let (dir, pool) = sample_folder("unconfirmed").await;
        let bytes = export(&pool, &dir, "1.2.3").await.unwrap();
        stage(&dir, bytes.into()).await.unwrap();
        pool.close().await;
        let before = state_of(&dir).await;
        assert!(!apply_pending(&dir, "1.2.3").await.unwrap());
        assert_eq!(state_of(&dir).await, before);
        assert!(leftovers(&dir).is_empty());
        let _ = fs::remove_dir_all(dir);
    }
}
