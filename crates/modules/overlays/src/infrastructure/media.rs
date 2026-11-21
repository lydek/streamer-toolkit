//! The media library's admin API and its read-only file route.

use std::sync::Arc;

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{
        DefaultBodyLimit, Path, Query, State,
        rejection::{BytesRejection, JsonRejection, QueryRejection},
    },
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use toolkit_core::events::Source;
use utoipa::{IntoParams, ToSchema};

use super::{ErrorBody, Overlays, error};
use crate::{
    application::{MediaError, MediaShow},
    domain::media::{MAX_BYTES, MediaEntry, MediaRule, Position},
};

pub(crate) const MEDIA_PLAY: &str = "screen.media";
pub(crate) const MEDIA_PLAY_VERSION: u32 = 1;

/// Where the alerts overlay shows a media item.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaPosition {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
    Full,
}

impl From<Position> for MediaPosition {
    fn from(p: Position) -> Self {
        match p {
            Position::TopLeft => Self::TopLeft,
            Position::TopRight => Self::TopRight,
            Position::BottomLeft => Self::BottomLeft,
            Position::BottomRight => Self::BottomRight,
            Position::Center => Self::Center,
            Position::Full => Self::Full,
        }
    }
}

impl From<MediaPosition> for Position {
    fn from(p: MediaPosition) -> Self {
        match p {
            MediaPosition::TopLeft => Self::TopLeft,
            MediaPosition::TopRight => Self::TopRight,
            MediaPosition::BottomLeft => Self::BottomLeft,
            MediaPosition::BottomRight => Self::BottomRight,
            MediaPosition::Center => Self::Center,
            MediaPosition::Full => Self::Full,
        }
    }
}

/// The `payload` of a `screen.media` event: the alerts overlay shows `url` at `position` for `durationMs`, a video
/// at `volume`. Not kept in snapshots, so a reload never shows it again.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MediaPlayPayload {
    pub url: String,
    /// `gif`, `webm` or `mp4`.
    pub format: String,
    pub position: MediaPosition,
    /// Percent.
    pub volume: u8,
    pub duration_ms: u32,
}

impl From<&MediaShow> for MediaPlayPayload {
    fn from(s: &MediaShow) -> Self {
        Self {
            url: format!("/media/{}", s.media_id),
            format: s.format.extension().into(),
            position: s.position.into(),
            volume: s.volume,
            duration_ms: s.duration_ms,
        }
    }
}

/// A `screen.media` event exactly as `/ws/overlay` sends it when its turn on the presentation queue comes, or at
/// once for 試播 (`source=simulation`).
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MediaPlayEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: MediaPlayType,
    pub version: u32,
    pub seq: u64,
    pub occurred_at: String,
    pub source: Source,
    pub payload: MediaPlayPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub enum MediaPlayType {
    #[serde(rename = "screen.media")]
    ScreenMedia,
}

/// What a channel points reward shows when redeemed.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MediaRuleBody {
    pub reward_id: String,
    /// A media library item.
    pub media_id: String,
    pub position: MediaPosition,
    /// 0–100 %; `null` follows the sound master volume.
    pub volume: Option<u8>,
}

impl From<MediaRule> for MediaRuleBody {
    fn from(r: MediaRule) -> Self {
        Self {
            reward_id: r.reward_id,
            media_id: r.media_id,
            position: r.position.into(),
            volume: r.volume,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MediaRuleInput {
    pub media_id: String,
    pub position: MediaPosition,
    /// 0–100 %; `null` follows the sound master volume.
    pub volume: Option<u8>,
}

/// One media item. `title` is broadcaster text: render it as text.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MediaItem {
    pub id: String,
    pub title: String,
    /// `gif`, `webm` or `mp4`.
    pub format: String,
    /// How long it shows; `null` until the admin window has read a video's length.
    pub duration_ms: Option<u32>,
    /// `/media/{id}`, readable without a token.
    pub url: String,
    /// The file is no longer in the data folder.
    pub missing: bool,
}

fn item(s: &Overlays, e: MediaEntry) -> MediaItem {
    MediaItem {
        missing: s.media_missing(&e),
        url: format!("/media/{}", e.id),
        id: e.id,
        title: e.title,
        format: e.format.extension().into(),
        duration_ms: e.duration_ms,
    }
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct MediaImportQuery {
    /// The original file name: its extension must match the content; the rest becomes the title.
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MediaUpdate {
    /// 1–100 characters after trimming.
    pub title: String,
    /// 1 ms to 10 minutes; `null` keeps the length it has.
    pub duration_ms: Option<u32>,
}

fn media_error(e: MediaError) -> Response {
    match e {
        MediaError::UnsupportedType => error(StatusCode::BAD_REQUEST, "media.unsupported_type"),
        MediaError::TooLarge => error(StatusCode::PAYLOAD_TOO_LARGE, "media.too_large"),
        MediaError::NotFound => error(StatusCode::NOT_FOUND, "media.not_found"),
        MediaError::Invalid => error(StatusCode::BAD_REQUEST, "media.invalid"),
        MediaError::SaveFailed(e) => {
            tracing::error!(code = "media.save_failed", error = %e, "media not saved");
            error(StatusCode::INTERNAL_SERVER_ERROR, "media.save_failed")
        }
    }
}

pub(crate) fn routes() -> Router<Arc<Overlays>> {
    Router::new()
        .route(
            "/api/admin/screen-tools/media",
            get(list_media)
                .post(import_media)
                .layer(DefaultBodyLimit::max(MAX_BYTES)),
        )
        .route(
            "/api/admin/screen-tools/media/{id}",
            put(update_media).delete(delete_media),
        )
        .route("/api/admin/screen-tools/media-rules", get(list_rules))
        .route(
            "/api/admin/screen-tools/media-rules/{reward_id}",
            put(set_rule).delete(delete_rule),
        )
        .route(
            "/api/admin/screen-tools/media-rules/{reward_id}/test",
            post(test_rule),
        )
}

/// What each reward shows.
#[utoipa::path(
    get,
    path = "/api/admin/screen-tools/media-rules",
    tag = "admin",
    responses((status = 200, body = Vec<MediaRuleBody>))
)]
pub(crate) async fn list_rules(State(s): State<Arc<Overlays>>) -> Json<Vec<MediaRuleBody>> {
    Json(s.media_rules().into_iter().map(Into::into).collect())
}

/// Sets what a reward shows.
#[utoipa::path(
    put,
    path = "/api/admin/screen-tools/media-rules/{reward_id}",
    tag = "admin",
    params(("reward_id" = String, Path)),
    request_body = MediaRuleInput,
    responses(
        (status = 200, body = MediaRuleBody),
        (status = 400, description = "`media.invalid`: a volume above 100", body = ErrorBody),
        (status = 404, description = "`media.not_found`: no such media item", body = ErrorBody),
        (status = 500, description = "`media.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn set_rule(
    State(s): State<Arc<Overlays>>,
    Path(reward_id): Path<String>,
    body: Result<Json<MediaRuleInput>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return error(StatusCode::BAD_REQUEST, "media.invalid");
    };
    let rule = MediaRule {
        reward_id,
        media_id: b.media_id,
        position: b.position.into(),
        volume: b.volume,
    };
    match s.set_media_rule(rule).await {
        Ok(rule) => Json(MediaRuleBody::from(rule)).into_response(),
        Err(e) => media_error(e),
    }
}

#[utoipa::path(
    delete,
    path = "/api/admin/screen-tools/media-rules/{reward_id}",
    tag = "admin",
    params(("reward_id" = String, Path)),
    responses(
        (status = 204),
        (status = 404, description = "`media.not_found`: the reward has no rule", body = ErrorBody),
        (status = 500, description = "`media.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn delete_rule(
    State(s): State<Arc<Overlays>>,
    Path(reward_id): Path<String>,
) -> Response {
    match s.delete_media_rule(&reward_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => media_error(e),
    }
}

/// 試播: shows the reward's media on the alerts overlay at once, as `source=simulation`; nothing is recorded.
#[utoipa::path(
    post,
    path = "/api/admin/screen-tools/media-rules/{reward_id}/test",
    tag = "admin",
    params(("reward_id" = String, Path)),
    responses(
        (status = 204),
        (status = 404, description = "`media.not_found`: no rule, its item is gone, or a video's length is not read yet", body = ErrorBody),
    )
)]
pub(crate) async fn test_rule(
    State(s): State<Arc<Overlays>>,
    Path(reward_id): Path<String>,
) -> Response {
    match s.test_media(&reward_id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => media_error(e),
    }
}

/// The read-only media file route for the alerts overlay and the admin UI; it needs no token.
pub fn file_routes(service: Arc<Overlays>) -> Router {
    Router::new()
        .route("/media/{id}", get(media_file))
        .with_state(service)
}

/// The media library in import order.
#[utoipa::path(
    get,
    path = "/api/admin/screen-tools/media",
    tag = "admin",
    responses(
        (status = 200, body = Vec<MediaItem>),
        (status = 500, description = "`media.save_failed`: the library cannot be read", body = ErrorBody),
    )
)]
pub(crate) async fn list_media(State(s): State<Arc<Overlays>>) -> Response {
    match s.media().await {
        Ok(all) => Json(all.into_iter().map(|e| item(&s, e)).collect::<Vec<_>>()).into_response(),
        Err(e) => media_error(e.into()),
    }
}

/// Copies a GIF, WebM or MP4 file (at most 50 MB) into the media library. The body is the file itself.
#[utoipa::path(
    post,
    path = "/api/admin/screen-tools/media",
    tag = "admin",
    params(MediaImportQuery),
    request_body(content = String, content_type = "application/octet-stream", description = "The media file"),
    responses(
        (status = 200, body = MediaItem),
        (status = 400, description = "`media.unsupported_type`: not a GIF, WebM or MP4 file (by extension and content)", body = ErrorBody),
        (status = 413, description = "`media.too_large`: larger than 50 MB", body = ErrorBody),
        (status = 500, description = "`media.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn import_media(
    State(s): State<Arc<Overlays>>,
    query: Result<Query<MediaImportQuery>, QueryRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Ok(Query(q)) = query else {
        return error(StatusCode::BAD_REQUEST, "media.unsupported_type");
    };
    let bytes = match body {
        Ok(b) => b,
        Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return media_error(MediaError::TooLarge);
        }
        Err(_) => return error(StatusCode::BAD_REQUEST, "media.unsupported_type"),
    };
    match s.import_media(&q.name, &bytes).await {
        Ok(entry) => Json(item(&s, entry)).into_response(),
        Err(e) => media_error(e),
    }
}

/// Renames an item and sets how long it shows.
#[utoipa::path(
    put,
    path = "/api/admin/screen-tools/media/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = MediaUpdate,
    responses(
        (status = 200, body = MediaItem),
        (status = 400, description = "`media.invalid`: an empty title or a length out of range", body = ErrorBody),
        (status = 404, description = "`media.not_found`", body = ErrorBody),
        (status = 500, description = "`media.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn update_media(
    State(s): State<Arc<Overlays>>,
    Path(id): Path<String>,
    body: Result<Json<MediaUpdate>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return error(StatusCode::BAD_REQUEST, "media.invalid");
    };
    match s.update_media(&id, &b.title, b.duration_ms).await {
        Ok(entry) => Json(item(&s, entry)).into_response(),
        Err(e) => media_error(e),
    }
}

#[utoipa::path(
    delete,
    path = "/api/admin/screen-tools/media/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    responses(
        (status = 204),
        (status = 404, description = "`media.not_found`", body = ErrorBody),
        (status = 500, description = "`media.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn delete_media(
    State(s): State<Arc<Overlays>>,
    Path(id): Path<String>,
) -> Response {
    match s.delete_media(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => media_error(e),
    }
}

/// An item's file with the Content-Type of its format, never sniffed.
// shortcut: the whole file, no byte ranges; enough for short clips played from the start. Add ranges (as
// `/music/{id}` has) if seeking is ever needed.
async fn media_file(State(s): State<Arc<Overlays>>, Path(id): Path<String>) -> Response {
    let Some((path, format)) = s.media_file(&id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut res = Response::new(Body::from(bytes));
    let headers = res.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(format.content_type()),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}
