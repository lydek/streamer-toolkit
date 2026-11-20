//! Adapters: the admin API (behind the admin token, added by the host) and the read-only sound file route.

use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

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
use toolkit_core::{
    bus::{Event, EventBus},
    events::Source,
    modules::Modules,
    presentation::Presenter,
    settings::SoundEntry,
};
use utoipa::{IntoParams, OpenApi, ToSchema};

use crate::{
    application::{SoundError, SoundService},
    domain::{Cue, MAX_BYTES},
};

/// Same shape as the host's admin error body: a stable `<area>.<reason>` code.
#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    code: String,
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(ErrorBody { code: code.into() })).into_response()
}

fn sound_error(e: SoundError) -> Response {
    let status = match &e {
        SoundError::UnsupportedType | SoundError::Invalid | SoundError::UnknownSlot => {
            StatusCode::BAD_REQUEST
        }
        SoundError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        SoundError::NotFound => StatusCode::NOT_FOUND,
        SoundError::SaveFailed(io) => {
            tracing::error!(code = "sound.save_failed", error = %io, "sound library not saved");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    error(status, &e.to_string())
}

/// One imported sound. `label` is broadcaster text: render it as text. Its file is `GET /sounds/{id}`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Sound {
    pub id: String,
    pub label: String,
    /// 0–100 (%).
    pub volume: u8,
    /// Slots that play it; deleting it puts them back to their defaults.
    pub used_by: Vec<String>,
    /// How long it plays; `null` until the admin window has measured it (`PUT /api/admin/sounds/{id}/duration`).
    #[schema(required)]
    pub duration_ms: Option<u32>,
}

fn sound(e: SoundEntry, slots: &BTreeMap<String, String>) -> Sound {
    Sound {
        used_by: slots
            .iter()
            .filter(|(_, id)| **id == e.id)
            .map(|(slot, _)| slot.clone())
            .collect(),
        id: e.id,
        label: e.label,
        volume: e.volume,
        duration_ms: e.duration_ms,
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SoundLibrary {
    pub sounds: Vec<Sound>,
    /// Chosen slots: `alert.<kind>`, `checkin`, `checkin.full` or `reward.<Twitch reward ID>` → a sound's ID or
    /// `silent`. A slot left out plays its default: the built-in sound for alerts, nothing otherwise.
    pub slots: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SlotChoice {
    /// A sound's ID, `silent`, or `null` for the slot's default.
    #[schema(required)]
    pub sound: Option<String>,
}

/// A library sound to play: `url` is its file (`/sounds/{id}`), `volume` 0–100 (%).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PlaySound {
    pub url: String,
    pub volume: u8,
}

impl PlaySound {
    /// `None` when nothing from the library plays.
    pub fn from_cue(cue: Cue) -> Option<Self> {
        match cue {
            Cue::File { id, volume } => Some(Self {
                url: format!("/sounds/{id}"),
                volume,
            }),
            Cue::BuiltIn | Cue::Silent => None,
        }
    }
}

const PLAY: &str = "sound.play";
const PLAY_VERSION: u32 = 1;

/// A `sound.play` event exactly as `/ws/overlay` sends it: a reward sound for `/overlay/sound/`, which plays them
/// one at a time in order. Not kept in snapshots, so nothing replays after a reload or restart.
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SoundPlayEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: SoundPlayType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: PlaySound,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum SoundPlayType {
    #[serde(rename = "sound.play")]
    SoundPlay,
}

impl SoundService {
    /// Plays reward sounds as `presenter` paces them, and test sounds at once, by publishing on `bus`.
    pub fn new(
        data_dir: PathBuf,
        modules: Arc<Modules>,
        bus: Arc<EventBus>,
        presenter: Arc<Presenter>,
    ) -> Self {
        Self::with_player(data_dir, modules, presenter, move |id, volume, source| {
            publish_play(&bus, id, volume, source)
        })
    }
}

fn publish_play(bus: &EventBus, id: &str, volume: u8, source: Source) {
    let payload = PlaySound::from_cue(Cue::File {
        id: id.into(),
        volume,
    });
    bus.publish(Event {
        id: uuid::Uuid::now_v7().to_string(),
        event_type: PLAY,
        version: PLAY_VERSION,
        occurred_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        source,
        payload: serde_json::to_value(payload).expect("serializable"),
    });
}

/// The sound an alert plays: the overlay's built-in one, none, or a library sound.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AlertSound {
    BuiltIn,
    Silent,
    File { url: String, volume: u8 },
}

impl From<Cue> for AlertSound {
    fn from(cue: Cue) -> Self {
        match cue {
            Cue::BuiltIn => Self::BuiltIn,
            Cue::Silent => Self::Silent,
            Cue::File { id, volume } => Self::File {
                url: format!("/sounds/{id}"),
                volume,
            },
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ImportQuery {
    /// The original file name: its extension must match the content; the rest becomes the label.
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SoundDuration {
    /// How long the sound plays, 1 ms to 6 hours.
    pub duration_ms: u32,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SoundUpdate {
    /// 1–100 characters after trimming.
    pub label: String,
    /// 0–100 (%).
    pub volume: u8,
}

/// The admin API.
pub fn routes(service: Arc<SoundService>) -> Router {
    Router::new()
        .route(
            "/api/admin/sounds",
            get(list_sounds)
                .post(import_sound)
                .layer(DefaultBodyLimit::max(MAX_BYTES)),
        )
        .route(
            "/api/admin/sounds/{id}",
            put(update_sound).delete(delete_sound),
        )
        .route("/api/admin/sounds/slots/{slot}", put(set_slot))
        .route("/api/admin/sounds/{id}/play", post(play_sound))
        .route("/api/admin/sounds/{id}/duration", put(set_duration))
        .with_state(service)
}

/// The read-only sound file route for overlays and the admin UI's preview; it needs no token.
pub fn file_routes(service: Arc<SoundService>) -> Router {
    Router::new()
        .route("/sounds/{id}", get(sound_file))
        .with_state(service)
}

#[utoipa::path(
    get,
    path = "/api/admin/sounds",
    tag = "admin",
    responses((status = 200, body = SoundLibrary))
)]
pub(crate) async fn list_sounds(State(s): State<Arc<SoundService>>) -> Json<SoundLibrary> {
    let slots = s.slots();
    Json(SoundLibrary {
        sounds: s.list().into_iter().map(|e| sound(e, &slots)).collect(),
        slots,
    })
}

/// Plays a sound once on `/overlay/sound/` (`source=simulation`), to test it in OBS; nothing is recorded.
#[utoipa::path(
    post,
    path = "/api/admin/sounds/{id}/play",
    tag = "admin",
    params(("id" = String, Path)),
    responses(
        (status = 204),
        (status = 404, description = "`sound.not_found`", body = ErrorBody),
    )
)]
pub(crate) async fn play_sound(
    State(s): State<Arc<SoundService>>,
    Path(id): Path<String>,
) -> Response {
    match s.play_test(&id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => sound_error(e),
    }
}

/// Chooses what a slot plays.
#[utoipa::path(
    put,
    path = "/api/admin/sounds/slots/{slot}",
    tag = "admin",
    params(("slot" = String, Path, description = "`alert.<kind>`, `checkin`, `checkin.full` or `reward.<Twitch reward ID>`")),
    request_body = SlotChoice,
    responses(
        (status = 204),
        (status = 400, description = "`sound.unknown_slot`", body = ErrorBody),
        (status = 404, description = "`sound.not_found`: no such sound", body = ErrorBody),
        (status = 500, description = "`sound.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn set_slot(
    State(s): State<Arc<SoundService>>,
    Path(slot): Path<String>,
    body: Result<Json<SlotChoice>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return error(StatusCode::BAD_REQUEST, "sound.invalid");
    };
    match s.set_slot(&slot, b.sound.as_deref()) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => sound_error(e),
    }
}

/// Copies an MP3, WAV or OGG file (at most 10 MB) into the library at 100% volume. The body is the file itself.
#[utoipa::path(
    post,
    path = "/api/admin/sounds",
    tag = "admin",
    params(ImportQuery),
    request_body(content = String, content_type = "application/octet-stream", description = "The sound file"),
    responses(
        (status = 200, body = Sound),
        (status = 400, description = "`sound.unsupported_type`: not an MP3, WAV or OGG file (by extension and content)", body = ErrorBody),
        (status = 413, description = "`sound.too_large`: larger than 10 MB", body = ErrorBody),
        (status = 500, description = "`sound.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn import_sound(
    State(s): State<Arc<SoundService>>,
    query: Result<Query<ImportQuery>, QueryRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Ok(Query(q)) = query else {
        return error(StatusCode::BAD_REQUEST, "sound.unsupported_type");
    };
    let bytes = match body {
        Ok(b) => b,
        Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return sound_error(SoundError::TooLarge);
        }
        Err(_) => return error(StatusCode::BAD_REQUEST, "sound.unsupported_type"),
    };
    match s.import(&q.name, &bytes) {
        Ok(entry) => Json(sound(entry, &BTreeMap::new())).into_response(),
        Err(e) => sound_error(e),
    }
}

/// Renames a sound or changes its volume.
#[utoipa::path(
    put,
    path = "/api/admin/sounds/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = SoundUpdate,
    responses(
        (status = 200, body = Sound),
        (status = 400, description = "`sound.invalid`: an empty label or a volume over 100", body = ErrorBody),
        (status = 404, description = "`sound.not_found`", body = ErrorBody),
        (status = 500, description = "`sound.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn update_sound(
    State(s): State<Arc<SoundService>>,
    Path(id): Path<String>,
    body: Result<Json<SoundUpdate>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return error(StatusCode::BAD_REQUEST, "sound.invalid");
    };
    match s.update(&id, &b.label, b.volume) {
        Ok(entry) => Json(sound(entry, &s.slots())).into_response(),
        Err(e) => sound_error(e),
    }
}

/// Records how long a sound plays; the admin window measures each sound after import, and older ones once.
#[utoipa::path(
    put,
    path = "/api/admin/sounds/{id}/duration",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = SoundDuration,
    responses(
        (status = 204),
        (status = 400, description = "`sound.invalid`: not between 1 ms and 6 hours", body = ErrorBody),
        (status = 404, description = "`sound.not_found`", body = ErrorBody),
        (status = 500, description = "`sound.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn set_duration(
    State(s): State<Arc<SoundService>>,
    Path(id): Path<String>,
    body: Result<Json<SoundDuration>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return error(StatusCode::BAD_REQUEST, "sound.invalid");
    };
    match s.set_duration(&id, b.duration_ms) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => sound_error(e),
    }
}

/// Deletes a sound and its file.
#[utoipa::path(
    delete,
    path = "/api/admin/sounds/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    responses(
        (status = 204),
        (status = 404, description = "`sound.not_found`", body = ErrorBody),
        (status = 500, description = "`sound.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn delete_sound(
    State(s): State<Arc<SoundService>>,
    Path(id): Path<String>,
) -> Response {
    match s.delete(&id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => sound_error(e),
    }
}

/// A library sound's file with the Content-Type of its format, never sniffed.
async fn sound_file(State(s): State<Arc<SoundService>>, Path(id): Path<String>) -> Response {
    let Some((path, format)) = s.file(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(bytes) = std::fs::read(&path) else {
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
#[openapi(
    paths(
        list_sounds,
        import_sound,
        update_sound,
        delete_sound,
        set_slot,
        play_sound,
        set_duration
    ),
    components(schemas(SoundPlayEvent))
)]
struct SoundApi;

/// This module's part of the Host's OpenAPI document.
pub fn openapi() -> utoipa::openapi::OpenApi {
    SoundApi::openapi()
}

#[cfg(test)]
mod tests;
