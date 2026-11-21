//! Adapters: the admin API (behind the admin token, added by the host) and the read-only music file route.

use std::{io::SeekFrom, sync::Arc};

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{
        DefaultBodyLimit, Path, Query, State,
        rejection::{BytesRejection, JsonRejection, QueryRejection},
    },
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncSeekExt},
    sync::watch,
};
use toolkit_core::{
    bus::{Event, EventBus},
    events::Source,
    modules::Modules,
    settings::{MusicEntry, Playlist},
};
use utoipa::{IntoParams, OpenApi, ToSchema};

use crate::{
    application::{
        BgmError, BgmService,
        player::{BgmPlayer, Command, PlayState, PlayerStatus},
    },
    domain::MAX_BYTES,
};

const PLAYER: &str = "bgm.player";
const PLAYER_VERSION: u32 = 1;

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
        BgmError::NothingToPlay | BgmError::Off => StatusCode::CONFLICT,
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

/// What to do with the player.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PlayerControl {
    /// Starts `playlist` (or the last chosen one) from its start, or resumes it when it is paused.
    Play {
        #[serde(default)]
        playlist: Option<String>,
    },
    Pause,
    Next,
    /// Within the first 3 seconds the track before; later the same track from its start.
    Previous,
    Stop,
    /// Moves within the track playing; past its end, the next one starts.
    Seek {
        position_ms: u64,
    },
}

impl From<PlayerControl> for Command {
    fn from(c: PlayerControl) -> Self {
        match c {
            PlayerControl::Play { playlist } => Self::Play { playlist },
            PlayerControl::Pause => Self::Pause,
            PlayerControl::Next => Self::Next,
            PlayerControl::Previous => Self::Previous,
            PlayerControl::Stop => Self::Stop,
            PlayerControl::Seek { position_ms } => Self::Seek { position_ms },
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PlayerOptions {
    /// 0–100 (%).
    pub volume: u8,
    /// Random order instead of the playlist's own.
    pub shuffle: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayStateName {
    Stopped,
    Playing,
    Paused,
}

/// The track playing. `title` is broadcaster text: render it as text.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlayerTrack {
    pub id: String,
    pub title: String,
    /// Its file: `/music/{id}`.
    pub url: String,
    pub duration_ms: u32,
}

/// The `payload` of a `bgm.player` event. The track is at `positionMs` at the event's `occurredAt`, and goes on
/// from there only while `state` is `playing` and `onAir` is true.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BgmPlayerPayload {
    pub state: PlayStateName,
    /// The playlist chosen last; kept after a stop.
    #[schema(required)]
    pub playlist: Option<String>,
    /// `null` while stopped.
    #[schema(required)]
    pub track: Option<PlayerTrack>,
    pub position_ms: u64,
    /// A music overlay is open in OBS; without one nothing plays and the position stands still.
    pub on_air: bool,
    pub shuffle: bool,
    /// 0–100 (%).
    pub volume: u8,
}

impl From<&PlayerStatus> for BgmPlayerPayload {
    fn from(s: &PlayerStatus) -> Self {
        Self {
            state: match s.state {
                PlayState::Stopped => PlayStateName::Stopped,
                PlayState::Playing => PlayStateName::Playing,
                PlayState::Paused => PlayStateName::Paused,
            },
            playlist: s.playlist.clone(),
            track: s.track.as_ref().map(|t| PlayerTrack {
                id: t.id.clone(),
                title: t.title.clone(),
                url: format!("/music/{}", t.id),
                duration_ms: t.duration_ms,
            }),
            position_ms: s.position_ms,
            on_air: s.on_air,
            shuffle: s.shuffle,
            volume: s.volume,
        }
    }
}

/// A `bgm.player` event exactly as `/ws/overlay` sends it; the snapshot keeps only the latest one.
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BgmPlayerEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: BgmPlayerType,
    pub version: u32,
    pub seq: u64,
    pub occurred_at: String,
    pub source: Source,
    pub payload: BgmPlayerPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub enum BgmPlayerType {
    #[serde(rename = "bgm.player")]
    BgmPlayer,
}

/// Starts the player, publishing every status as `bgm.player` (kept in snapshots); needs a Tokio runtime.
/// `on_air` says whether a music overlay is open, re-read whenever `changes` ticks.
pub fn start(
    service: Arc<BgmService>,
    modules: Arc<Modules>,
    bus: Arc<EventBus>,
    on_air: impl Fn() -> bool + Send + Sync + 'static,
    changes: watch::Receiver<()>,
) -> Arc<BgmPlayer> {
    bus.retain(PLAYER, 1);
    BgmPlayer::start(service, modules, on_air, changes, move |status| {
        bus.publish(Event {
            id: uuid::Uuid::now_v7().to_string(),
            event_type: PLAYER,
            version: PLAYER_VERSION,
            occurred_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            source: Source::System,
            payload: serde_json::to_value(BgmPlayerPayload::from(status))
                .expect("player status serializes"),
        });
    })
}

/// The admin API.
pub fn routes(service: Arc<BgmService>, player: Arc<BgmPlayer>) -> Router {
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
        .merge(
            Router::new()
                .route("/api/admin/music/player", post(control_player))
                .route("/api/admin/music/player/options", put(set_player_options))
                .with_state(player),
        )
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

/// Plays, pauses, skips, stops or seeks; the result arrives as a `bgm.player` event.
#[utoipa::path(
    post,
    path = "/api/admin/music/player",
    tag = "admin",
    request_body = PlayerControl,
    responses(
        (status = 204),
        (status = 400, description = "`music.invalid`: no playlist chosen yet", body = ErrorBody),
        (status = 404, description = "`music.not_found`: no such playlist", body = ErrorBody),
        (status = 409, description = "`music.nothing_to_play`: none of its tracks can play (files gone, lengths not measured); `music.off`: the feature is turned off", body = ErrorBody),
    )
)]
pub(crate) async fn control_player(
    State(player): State<Arc<BgmPlayer>>,
    body: Result<Json<PlayerControl>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return error(StatusCode::BAD_REQUEST, "music.invalid");
    };
    match player.command(b.into()) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => bgm_error(e),
    }
}

/// Sets the music volume and whether playlists play shuffled.
#[utoipa::path(
    put,
    path = "/api/admin/music/player/options",
    tag = "admin",
    request_body = PlayerOptions,
    responses(
        (status = 204),
        (status = 400, description = "`music.invalid`: a volume over 100", body = ErrorBody),
        (status = 500, description = "`music.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn set_player_options(
    State(player): State<Arc<BgmPlayer>>,
    body: Result<Json<PlayerOptions>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return error(StatusCode::BAD_REQUEST, "music.invalid");
    };
    match player.set_options(b.volume, b.shuffle) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => bgm_error(e),
    }
}

/// One `bytes=` range as `[start, end)` within `size`; `None` when it cannot be served.
// shortcut: a single range only, which is what browsers ask for when playing media.
fn byte_range(value: &str, size: u64) -> Option<(u64, u64)> {
    let (first, last) = value.strip_prefix("bytes=")?.split_once('-')?;
    let (start, end) = match (first.parse::<u64>(), last.parse::<u64>()) {
        // The last `n` bytes.
        (Err(_), Ok(n)) if first.is_empty() => (size.saturating_sub(n), size),
        (Ok(start), Err(_)) if last.is_empty() => (start, size),
        (Ok(start), Ok(last)) => (start, last.saturating_add(1).min(size)),
        _ => return None,
    };
    (start < end).then_some((start, end))
}

/// A track's file with the Content-Type of its format, never sniffed. Answers byte ranges, so the overlay can start
/// in the middle of a track.
async fn music_file(
    State(s): State<Arc<BgmService>>,
    Path(id): Path<String>,
    request: HeaderMap,
) -> Response {
    let Some((path, format)) = s.file(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(mut file) = tokio::fs::File::open(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(size) = file.metadata().await.map(|m| m.len()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let range = match request.get(header::RANGE) {
        None => None,
        Some(v) => match v.to_str().ok().and_then(|v| byte_range(v, size)) {
            Some(r) => Some(r),
            None => {
                let mut res = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
                if let Ok(v) = HeaderValue::from_str(&format!("bytes */{size}")) {
                    res.headers_mut().insert(header::CONTENT_RANGE, v);
                }
                return res;
            }
        },
    };
    let (start, end) = range.unwrap_or((0, size));
    let mut bytes = Vec::new();
    if file.seek(SeekFrom::Start(start)).await.is_err()
        || file
            .take(end - start)
            .read_to_end(&mut bytes)
            .await
            .is_err()
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    let mut res = Response::new(Body::from(bytes));
    if range.is_some() {
        *res.status_mut() = StatusCode::PARTIAL_CONTENT;
        if let Ok(v) = HeaderValue::from_str(&format!("bytes {start}-{}/{size}", end - 1)) {
            res.headers_mut().insert(header::CONTENT_RANGE, v);
        }
    }
    let headers = res.headers_mut();
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
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
#[openapi(
    paths(
        list_music,
        import_music,
        update_track,
        set_duration,
        delete_track,
        set_playlists,
        create_playlist,
        rename_playlist,
        delete_playlist,
        control_player,
        set_player_options
    ),
    components(schemas(BgmPlayerEvent))
)]
struct BgmApi;

/// This module's part of the Host's OpenAPI document.
pub fn openapi() -> utoipa::openapi::OpenApi {
    BgmApi::openapi()
}

#[cfg(test)]
mod tests;
