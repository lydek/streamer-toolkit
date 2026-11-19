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
mod tests {
    use super::*;
    use axum::http::Request;
    use std::path::PathBuf;
    use tower::ServiceExt;

    pub(crate) const WAV: &[u8] = b"RIFF\x24\0\0\0WAVEfmt \x10\0\0\0";

    fn data_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-sound-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn app(dir: &std::path::Path) -> Router {
        let modules = Arc::new(toolkit_core::modules::Modules::load(dir.to_path_buf()));
        let service = Arc::new(SoundService::new(
            dir.to_path_buf(),
            modules,
            Arc::new(EventBus::default()),
            toolkit_core::presentation::Presenter::start(
                toolkit_core::presentation::DEFAULT_GAP,
                |_| true,
            ),
        ));
        routes(service.clone()).merge(file_routes(service))
    }

    async fn send(app: &Router, req: Request<Body>) -> (StatusCode, Response) {
        let res = app.clone().oneshot(req).await.unwrap();
        (res.status(), res)
    }

    async fn json(res: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn upload(name: &str, body: impl Into<Body>) -> Request<Body> {
        Request::post(format!("/api/admin/sounds?name={name}"))
            .body(body.into())
            .unwrap()
    }

    #[tokio::test]
    async fn import_copies_the_file_under_a_new_name() {
        let dir = data_dir("import");
        let app = app(&dir);
        let (status, res) = send(&app, upload("..%2F..%2Fbell.WAV", WAV)).await;
        assert_eq!(status, StatusCode::OK);
        let sound = json(res).await;
        assert_eq!(sound["label"], "bell");
        assert_eq!(sound["volume"], 100);
        let id = sound["id"].as_str().unwrap();
        assert_eq!(uuid::Uuid::parse_str(id).unwrap().get_version_num(), 7);
        let files: Vec<_> = std::fs::read_dir(dir.join("sounds"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(
            files,
            [format!("{id}.wav")],
            "named by the ID, never the file name"
        );
        assert!(!dir.join("bell.WAV").exists() && !dir.parent().unwrap().join("bell.WAV").exists());

        let (status, res) = send(
            &app,
            Request::get(format!("/sounds/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let h = res.headers();
        assert_eq!(h[header::CONTENT_TYPE], "audio/wav");
        assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], WAV);

        let (_, res) = send(
            &app,
            Request::get("/api/admin/sounds")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(json(res).await["sounds"][0]["id"], id);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn only_mp3_wav_and_ogg_up_to_10_mb_are_taken() {
        let dir = data_dir("refuse");
        let app = app(&dir);
        for (name, body) in [
            ("x.mp3", WAV.to_vec()),
            ("x.html", b"<script>".to_vec()),
            ("x.wav", Vec::new()),
        ] {
            let (status, res) = send(&app, upload(name, body)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{name}");
            assert_eq!(json(res).await["code"], "sound.unsupported_type");
        }
        let mut big = WAV.to_vec();
        big.resize(MAX_BYTES + 1, 0);
        let (status, res) = send(&app, upload("x.wav", big.clone())).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(json(res).await["code"], "sound.too_large");
        big.truncate(MAX_BYTES);
        assert_eq!(
            send(&app, upload("x.wav", big)).await.0,
            StatusCode::OK,
            "exactly 10 MB"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn rename_change_volume_and_delete() {
        let dir = data_dir("edit");
        let app = app(&dir);
        let id = json(send(&app, upload("a.wav", WAV)).await.1).await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let put = |body: serde_json::Value| {
            Request::put(format!("/api/admin/sounds/{id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap()
        };
        let (status, res) = send(
            &app,
            put(serde_json::json!({ "label": " Ding ", "volume": 40 })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            json(res).await,
            serde_json::json!({ "id": id, "label": "Ding", "volume": 40, "used_by": [], "duration_ms": null })
        );
        for bad in [
            serde_json::json!({ "label": " ", "volume": 40 }),
            serde_json::json!({ "label": "a", "volume": 101 }),
        ] {
            assert_eq!(send(&app, put(bad)).await.0, StatusCode::BAD_REQUEST);
        }

        let delete = || {
            Request::delete(format!("/api/admin/sounds/{id}"))
                .body(Body::empty())
                .unwrap()
        };
        assert_eq!(send(&app, delete()).await.0, StatusCode::NO_CONTENT);
        assert_eq!(send(&app, delete()).await.0, StatusCode::NOT_FOUND);
        assert_eq!(
            std::fs::read_dir(dir.join("sounds")).unwrap().count(),
            0,
            "file removed"
        );
        let file = Request::get(format!("/sounds/{id}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(&app, file).await.0, StatusCode::NOT_FOUND);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn the_file_route_serves_library_sounds_only() {
        let dir = data_dir("route");
        let app = app(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), "{}").unwrap();
        for path in [
            "/sounds/settings.json",
            "/sounds/..%2Fsettings.json",
            "/sounds/x",
        ] {
            let req = Request::get(path).body(Body::empty()).unwrap();
            assert_eq!(send(&app, req).await.0, StatusCode::NOT_FOUND, "{path}");
        }
        // An edited settings.json cannot point the route outside `sounds/`.
        toolkit_core::settings::update(&dir, |s| {
            s.sound.library.push(SoundEntry {
                id: "../settings".into(),
                label: "x".into(),
                volume: 100,
                format: "json".into(),
                duration_ms: None,
            })
        })
        .unwrap();
        let req = Request::get("/sounds/..%2Fsettings")
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(&app, req).await.0, StatusCode::NOT_FOUND);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn choose(slot: &str, sound: serde_json::Value) -> Request<Body> {
        Request::put(format!("/api/admin/sounds/slots/{slot}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({ "sound": sound }).to_string(),
            ))
            .unwrap()
    }

    #[tokio::test]
    async fn slots_play_their_choice_and_fall_back_when_the_sound_is_deleted() {
        let dir = data_dir("slots");
        let modules = Arc::new(toolkit_core::modules::Modules::load(dir.clone()));
        let service = Arc::new(SoundService::new(
            dir.clone(),
            modules.clone(),
            Arc::new(EventBus::default()),
            toolkit_core::presentation::Presenter::start(
                toolkit_core::presentation::DEFAULT_GAP,
                |_| true,
            ),
        ));
        let app = routes(service.clone());
        let id = json(send(&app, upload("a.wav", WAV)).await.1).await["id"]
            .as_str()
            .unwrap()
            .to_owned();

        // Nothing chosen: alerts keep the built-in sound, check-ins are silent.
        assert_eq!(
            AlertSound::from(service.alert("follow")),
            AlertSound::BuiltIn
        );
        assert_eq!(PlaySound::from_cue(service.checkin(false)), None);

        for (slot, choice) in [
            ("alert.follow", id.as_str()),
            ("checkin.full", id.as_str()),
            ("alert.raid", "silent"),
        ] {
            assert_eq!(
                send(&app, choose(slot, choice.into())).await.0,
                StatusCode::NO_CONTENT,
                "{slot}"
            );
        }
        let file = AlertSound::File {
            url: format!("/sounds/{id}"),
            volume: 100,
        };
        assert_eq!(AlertSound::from(service.alert("follow")), file);
        assert_eq!(AlertSound::from(service.alert("raid")), AlertSound::Silent);
        assert_eq!(
            AlertSound::from(service.alert("cheer")),
            AlertSound::BuiltIn
        );
        assert_eq!(
            PlaySound::from_cue(service.checkin(true)),
            Some(PlaySound {
                url: format!("/sounds/{id}"),
                volume: 100
            })
        );
        assert_eq!(PlaySound::from_cue(service.checkin(false)), None);

        let (_, res) = send(
            &app,
            Request::get("/api/admin/sounds")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        let library = json(res).await;
        assert_eq!(
            library["sounds"][0]["used_by"],
            serde_json::json!(["alert.follow", "checkin.full"])
        );
        assert_eq!(library["slots"]["alert.raid"], "silent");

        for (slot, choice, status, code) in [
            (
                "alert.host",
                serde_json::json!(id),
                StatusCode::BAD_REQUEST,
                "sound.unknown_slot",
            ),
            (
                "checkin.half",
                serde_json::Value::Null,
                StatusCode::BAD_REQUEST,
                "sound.unknown_slot",
            ),
            (
                "checkin",
                serde_json::json!("nope"),
                StatusCode::NOT_FOUND,
                "sound.not_found",
            ),
        ] {
            let (got, res) = send(&app, choose(slot, choice)).await;
            assert_eq!(
                (got, json(res).await["code"].clone()),
                (status, code.into()),
                "{slot}"
            );
        }

        // While the module is off every slot plays its default.
        modules
            .set_enabled(toolkit_core::modules::SOUND, false)
            .unwrap();
        assert_eq!(
            AlertSound::from(service.alert("follow")),
            AlertSound::BuiltIn
        );
        assert_eq!(PlaySound::from_cue(service.checkin(true)), None);
        modules
            .set_enabled(toolkit_core::modules::SOUND, true)
            .unwrap();

        let delete = Request::delete(format!("/api/admin/sounds/{id}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(&app, delete).await.0, StatusCode::NO_CONTENT);
        assert_eq!(
            AlertSound::from(service.alert("follow")),
            AlertSound::BuiltIn
        );
        assert_eq!(PlaySound::from_cue(service.checkin(true)), None);
        assert_eq!(
            service.slots(),
            BTreeMap::from([("alert.raid".to_owned(), "silent".to_owned())]),
            "only the slots that played it are reset"
        );
        // Back to the default with `null`.
        assert_eq!(
            send(&app, choose("alert.raid", serde_json::Value::Null))
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        assert!(service.slots().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn the_admin_window_records_each_sounds_length() {
        let dir = data_dir("duration");
        let app = app(&dir);
        let id = json(send(&app, upload("bell.wav", WAV)).await.1).await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let set = |id: &str, body: serde_json::Value| {
            Request::put(format!("/api/admin/sounds/{id}/duration"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap()
        };
        let list = || {
            Request::get("/api/admin/sounds")
                .body(Body::empty())
                .unwrap()
        };
        assert_eq!(
            json(send(&app, list()).await.1).await["sounds"][0]["duration_ms"],
            serde_json::Value::Null,
            "unmeasured after import"
        );
        let ok = send(&app, set(&id, serde_json::json!({"duration_ms": 1500}))).await;
        assert_eq!(ok.0, StatusCode::NO_CONTENT);
        assert_eq!(
            json(send(&app, list()).await.1).await["sounds"][0]["duration_ms"],
            1500
        );
        for bad in [
            serde_json::json!({"duration_ms": 0}),
            serde_json::json!({"duration_ms": 6 * 60 * 60 * 1000 + 1}),
            serde_json::json!({"duration_ms": "1500"}),
        ] {
            assert_eq!(send(&app, set(&id, bad)).await.0, StatusCode::BAD_REQUEST);
        }
        let missing = set("nope", serde_json::json!({"duration_ms": 1500}));
        assert_eq!(send(&app, missing).await.0, StatusCode::NOT_FOUND);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_reward_sound_holds_the_queue_for_its_length() {
        use toolkit_core::events::RedemptionChange::Added;
        let dir = data_dir("reward-length");
        let bus = Arc::new(EventBus::default());
        let modules = Arc::new(toolkit_core::modules::Modules::load(dir.clone()));
        let gap = std::time::Duration::from_millis(10);
        let service = Arc::new(SoundService::new(
            dir.clone(),
            modules,
            bus.clone(),
            toolkit_core::presentation::Presenter::start(gap, |_| true),
        ));
        let app = routes(service.clone());
        let id = json(send(&app, upload("bell.wav", WAV)).await.1).await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let reward = "92af127c-7326-4483-a52b-b0da0be61c01";
        send(&app, choose(&format!("reward.{reward}"), id.clone().into())).await;
        service.set_duration(&id, 300).unwrap();
        let (_, mut rx) = bus.subscribe();

        service.receive_redemption(&redemption(reward, Added));
        service.receive_redemption(&redemption(reward, Added));
        rx.recv().await.unwrap();
        let first = std::time::Instant::now();
        rx.recv().await.unwrap();
        assert!(
            first.elapsed() >= std::time::Duration::from_millis(300),
            "the second waits until the first has played"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_reward_sound_whose_file_is_gone_is_marked_and_plays_nothing() {
        use toolkit_core::events::RedemptionChange::Added;
        use toolkit_core::presentation::{Problem, Subject};
        let dir = data_dir("reward-missing");
        let bus = Arc::new(EventBus::default());
        let modules = Arc::new(toolkit_core::modules::Modules::load(dir.clone()));
        let presenter =
            toolkit_core::presentation::Presenter::start(std::time::Duration::ZERO, |_| true);
        let mut status = presenter.subscribe();
        let service = Arc::new(SoundService::new(
            dir.clone(),
            modules,
            bus.clone(),
            presenter,
        ));
        let app = routes(service.clone());
        let id = json(send(&app, upload("bell.wav", WAV)).await.1).await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let reward = "92af127c-7326-4483-a52b-b0da0be61c01";
        send(&app, choose(&format!("reward.{reward}"), id.clone().into())).await;
        assert!(!service.file_missing(&id));
        std::fs::remove_file(service.file(&id).unwrap().0).unwrap();
        assert!(service.file_missing(&id));
        let (_, mut rx) = bus.subscribe();

        service.receive_redemption(&redemption(reward, Added));
        let playing = status
            .wait_for(|s| s.current.as_ref().is_some_and(|c| c.problem.is_some()))
            .await
            .unwrap()
            .current
            .clone()
            .unwrap();
        assert_eq!(playing.problem, Some(Problem::SoundMissing));
        assert_eq!(
            playing.subject,
            Subject::Sound {
                sound: "bell".into(),
                viewer: "Viewer".into()
            }
        );
        assert!(rx.try_recv().is_err(), "nothing sent to the sound overlay");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn redemption(
        reward: &str,
        change: toolkit_core::events::RedemptionChange,
    ) -> toolkit_core::events::ChannelPointsRedemption {
        toolkit_core::events::ChannelPointsRedemption {
            id: "r-1".into(),
            change,
            user_id: "1".into(),
            user_login: "viewer".into(),
            user_name: "Viewer".into(),
            reward_id: reward.into(),
            reward_title: "Bell".into(),
            reward_cost: 100,
            user_input: String::new(),
            status: toolkit_core::events::RedemptionStatus::Unfulfilled,
            redeemed_at: "2026-11-17T10:00:00Z".into(),
        }
    }

    #[tokio::test]
    async fn new_redemptions_of_a_mapped_reward_play_its_sound_once_and_live_only() {
        use toolkit_core::events::RedemptionChange::{Added, Updated};
        let dir = data_dir("rewards");
        let bus = Arc::new(EventBus::default());
        let modules = Arc::new(toolkit_core::modules::Modules::load(dir.clone()));
        let service = Arc::new(SoundService::new(
            dir.clone(),
            modules.clone(),
            bus.clone(),
            toolkit_core::presentation::Presenter::start(
                toolkit_core::presentation::DEFAULT_GAP,
                |_| true,
            ),
        ));
        let app = routes(service.clone());
        let id = json(send(&app, upload("bell.wav", WAV)).await.1).await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let reward = "92af127c-7326-4483-a52b-b0da0be61c01";
        assert_eq!(
            send(&app, choose(&format!("reward.{reward}"), id.clone().into()))
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        let (_, mut rx) = bus.subscribe();

        service.receive_redemption(&redemption(reward, Updated));
        service.receive_redemption(&redemption("other-reward", Added));
        modules
            .set_enabled(toolkit_core::modules::SOUND, false)
            .unwrap();
        service.receive_redemption(&redemption(reward, Added));
        modules
            .set_enabled(toolkit_core::modules::SOUND, true)
            .unwrap();
        assert!(
            rx.try_recv().is_err(),
            "status changes, other rewards and a disabled module play nothing"
        );

        service.receive_redemption(&redemption(reward, Added));
        // The presentation queue publishes it on its own task.
        let event: serde_json::Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
        assert_eq!(event["type"], "sound.play");
        assert_eq!(event["source"], "twitch");
        assert_eq!(
            event["payload"],
            serde_json::json!({ "url": format!("/sounds/{id}"), "volume": 100 })
        );
        let typed: SoundPlayEvent = serde_json::from_value(event.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(typed).unwrap(),
            event,
            "the schema type matches the bus"
        );
        let (snapshot, _) = bus.subscribe();
        assert!(
            !snapshot.contains("sound.play"),
            "never replayed to an overlay that connects later"
        );

        // The test button plays any library sound as a simulation.
        let play = |id: &str| {
            Request::post(format!("/api/admin/sounds/{id}/play"))
                .body(Body::empty())
                .unwrap()
        };
        assert_eq!(send(&app, play(&id)).await.0, StatusCode::NO_CONTENT);
        let event: serde_json::Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(
            (event["type"].as_str(), event["source"].as_str()),
            (Some("sound.play"), Some("simulation"))
        );
        assert_eq!(send(&app, play("nope")).await.0, StatusCode::NOT_FOUND);

        let library = json(
            send(
                &app,
                Request::get("/api/admin/sounds")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .1,
        )
        .await;
        assert_eq!(
            library["sounds"][0]["used_by"],
            serde_json::json!([format!("reward.{reward}")])
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
