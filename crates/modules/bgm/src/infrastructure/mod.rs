//! Adapters: the admin API (behind the admin token, added by the host) and the read-only music file route.

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
use toolkit_core::settings::{MusicEntry, Playlist};
use utoipa::{IntoParams, OpenApi, ToSchema};

use crate::{
    application::{BgmError, BgmService},
    domain::MAX_BYTES,
};

/// Same shape as the host's admin error body: a stable `<area>.<reason>` code.
#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    code: String,
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(ErrorBody { code: code.into() })).into_response()
}

fn bgm_error(e: BgmError) -> Response {
    let status = match &e {
        BgmError::UnsupportedType | BgmError::Invalid => StatusCode::BAD_REQUEST,
        BgmError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        BgmError::NotFound => StatusCode::NOT_FOUND,
        BgmError::SaveFailed(io) => {
            tracing::error!(code = "music.save_failed", error = %io, "music library not saved");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    error(status, &e.to_string())
}

/// One imported track. `title` is broadcaster text: render it as text. Its file is `GET /music/{id}`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Track {
    pub id: String,
    pub title: String,
    /// How long it plays; `null` until the admin window has measured it (`PUT /api/admin/music/{id}/duration`).
    #[schema(required)]
    pub duration_ms: Option<u32>,
    /// Its file is gone from the music folder (deleted outside the app).
    pub missing: bool,
}

fn track(s: &BgmService, e: MusicEntry) -> Track {
    Track {
        missing: s.file_missing(&e),
        id: e.id,
        title: e.title,
        duration_ms: e.duration_ms,
    }
}

/// A playlist. `name` is broadcaster text: render it as text.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MusicPlaylist {
    pub id: String,
    pub name: String,
    /// Track IDs in play order.
    pub tracks: Vec<String>,
}

impl From<Playlist> for MusicPlaylist {
    fn from(p: Playlist) -> Self {
        Self {
            id: p.id,
            name: p.name,
            tracks: p.tracks,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MusicLibrary {
    /// In import order.
    pub tracks: Vec<Track>,
    pub playlists: Vec<MusicPlaylist>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ImportQuery {
    /// The original file name: its extension must match the content; the rest becomes the title.
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TrackUpdate {
    /// 1–100 characters after trimming.
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TrackDuration {
    /// How long the track plays, 1 ms to 24 hours.
    pub duration_ms: u32,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TrackPlaylists {
    /// Every playlist the track should be in; it joins the end of new ones and leaves the others.
    pub playlists: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PlaylistName {
    /// 1–100 characters after trimming.
    pub name: String,
}

/// The admin API.
pub fn routes(service: Arc<BgmService>) -> Router {
    Router::new()
        .route(
            "/api/admin/music",
            get(list_music)
                .post(import_music)
                .layer(DefaultBodyLimit::max(MAX_BYTES)),
        )
        .route(
            "/api/admin/music/{id}",
            put(update_track).delete(delete_track),
        )
        .route("/api/admin/music/{id}/duration", put(set_duration))
        .route("/api/admin/music/{id}/playlists", put(set_playlists))
        .route("/api/admin/music/playlists", post(create_playlist))
        .route(
            "/api/admin/music/playlists/{id}",
            put(rename_playlist).delete(delete_playlist),
        )
        .with_state(service)
}

/// The read-only music file route for the music overlay and the admin UI's preview; it needs no token.
pub fn file_routes(service: Arc<BgmService>) -> Router {
    Router::new()
        .route("/music/{id}", get(music_file))
        .with_state(service)
}

#[utoipa::path(
    get,
    path = "/api/admin/music",
    tag = "admin",
    responses((status = 200, body = MusicLibrary))
)]
pub(crate) async fn list_music(State(s): State<Arc<BgmService>>) -> Json<MusicLibrary> {
    let library = s.library();
    Json(MusicLibrary {
        tracks: library.library.into_iter().map(|e| track(&s, e)).collect(),
        playlists: library.playlists.into_iter().map(Into::into).collect(),
    })
}

/// Copies an MP3, WAV or OGG file (at most 50 MB) into the music library. The body is the file itself.
#[utoipa::path(
    post,
    path = "/api/admin/music",
    tag = "admin",
    params(ImportQuery),
    request_body(content = String, content_type = "application/octet-stream", description = "The music file"),
    responses(
        (status = 200, body = Track),
        (status = 400, description = "`music.unsupported_type`: not an MP3, WAV or OGG file (by extension and content)", body = ErrorBody),
        (status = 413, description = "`music.too_large`: larger than 50 MB", body = ErrorBody),
        (status = 500, description = "`music.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn import_music(
    State(s): State<Arc<BgmService>>,
    query: Result<Query<ImportQuery>, QueryRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Ok(Query(q)) = query else {
        return error(StatusCode::BAD_REQUEST, "music.unsupported_type");
    };
    let bytes = match body {
        Ok(b) => b,
        Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return bgm_error(BgmError::TooLarge);
        }
        Err(_) => return error(StatusCode::BAD_REQUEST, "music.unsupported_type"),
    };
    match s.import(&q.name, &bytes) {
        Ok(entry) => Json(track(&s, entry)).into_response(),
        Err(e) => bgm_error(e),
    }
}

/// Renames a track.
#[utoipa::path(
    put,
    path = "/api/admin/music/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = TrackUpdate,
    responses(
        (status = 200, body = Track),
        (status = 400, description = "`music.invalid`: an empty title", body = ErrorBody),
        (status = 404, description = "`music.not_found`", body = ErrorBody),
        (status = 500, description = "`music.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn update_track(
    State(s): State<Arc<BgmService>>,
    Path(id): Path<String>,
    body: Result<Json<TrackUpdate>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return error(StatusCode::BAD_REQUEST, "music.invalid");
    };
    match s.rename(&id, &b.title) {
        Ok(entry) => Json(track(&s, entry)).into_response(),
        Err(e) => bgm_error(e),
    }
}

/// Records how long a track plays; the admin window measures each track after import, and unmeasured ones when
/// the music page opens.
#[utoipa::path(
    put,
    path = "/api/admin/music/{id}/duration",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = TrackDuration,
    responses(
        (status = 204),
        (status = 400, description = "`music.invalid`: not between 1 ms and 24 hours", body = ErrorBody),
        (status = 404, description = "`music.not_found`", body = ErrorBody),
        (status = 500, description = "`music.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn set_duration(
    State(s): State<Arc<BgmService>>,
    Path(id): Path<String>,
    body: Result<Json<TrackDuration>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return error(StatusCode::BAD_REQUEST, "music.invalid");
    };
    match s.set_duration(&id, b.duration_ms) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => bgm_error(e),
    }
}

/// Deletes a track, takes it out of every playlist, and deletes its file.
#[utoipa::path(
    delete,
    path = "/api/admin/music/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    responses(
        (status = 204),
        (status = 404, description = "`music.not_found`", body = ErrorBody),
        (status = 500, description = "`music.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn delete_track(
    State(s): State<Arc<BgmService>>,
    Path(id): Path<String>,
) -> Response {
    match s.delete(&id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => bgm_error(e),
    }
}

/// Chooses which playlists a track is in.
#[utoipa::path(
    put,
    path = "/api/admin/music/{id}/playlists",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = TrackPlaylists,
    responses(
        (status = 204),
        (status = 400, description = "`music.invalid`", body = ErrorBody),
        (status = 404, description = "`music.not_found`: no such track or playlist", body = ErrorBody),
        (status = 500, description = "`music.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn set_playlists(
    State(s): State<Arc<BgmService>>,
    Path(id): Path<String>,
    body: Result<Json<TrackPlaylists>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return error(StatusCode::BAD_REQUEST, "music.invalid");
    };
    match s.set_playlists(&id, &b.playlists) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => bgm_error(e),
    }
}

/// Adds an empty playlist at the end.
#[utoipa::path(
    post,
    path = "/api/admin/music/playlists",
    tag = "admin",
    request_body = PlaylistName,
    responses(
        (status = 200, body = MusicPlaylist),
        (status = 400, description = "`music.invalid`: an empty name", body = ErrorBody),
        (status = 500, description = "`music.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn create_playlist(
    State(s): State<Arc<BgmService>>,
    body: Result<Json<PlaylistName>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return error(StatusCode::BAD_REQUEST, "music.invalid");
    };
    match s.create_playlist(&b.name) {
        Ok(p) => Json(MusicPlaylist::from(p)).into_response(),
        Err(e) => bgm_error(e),
    }
}

/// Renames a playlist.
#[utoipa::path(
    put,
    path = "/api/admin/music/playlists/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = PlaylistName,
    responses(
        (status = 200, body = MusicPlaylist),
        (status = 400, description = "`music.invalid`: an empty name", body = ErrorBody),
        (status = 404, description = "`music.not_found`", body = ErrorBody),
        (status = 500, description = "`music.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn rename_playlist(
    State(s): State<Arc<BgmService>>,
    Path(id): Path<String>,
    body: Result<Json<PlaylistName>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return error(StatusCode::BAD_REQUEST, "music.invalid");
    };
    match s.rename_playlist(&id, &b.name) {
        Ok(p) => Json(MusicPlaylist::from(p)).into_response(),
        Err(e) => bgm_error(e),
    }
}

/// Deletes a playlist; its tracks stay in the library.
#[utoipa::path(
    delete,
    path = "/api/admin/music/playlists/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    responses(
        (status = 204),
        (status = 404, description = "`music.not_found`", body = ErrorBody),
        (status = 500, description = "`music.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn delete_playlist(
    State(s): State<Arc<BgmService>>,
    Path(id): Path<String>,
) -> Response {
    match s.delete_playlist(&id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => bgm_error(e),
    }
}

/// A track's file with the Content-Type of its format, never sniffed.
async fn music_file(State(s): State<Arc<BgmService>>, Path(id): Path<String>) -> Response {
    let Some((path, format)) = s.file(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // shortcut: the whole file at once, no Range requests; add them when the player needs to seek (P2-13b).
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

#[derive(OpenApi)]
#[openapi(paths(
    list_music,
    import_music,
    update_track,
    set_duration,
    delete_track,
    set_playlists,
    create_playlist,
    rename_playlist,
    delete_playlist
))]
struct BgmApi;

/// This module's part of the Host's OpenAPI document.
pub fn openapi() -> utoipa::openapi::OpenApi {
    BgmApi::openapi()
}

#[cfg(test)]
mod tests;
