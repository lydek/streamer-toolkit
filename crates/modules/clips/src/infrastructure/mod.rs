//! Adapters: the admin API (behind the admin token, added by the host) and the `clip.state` overlay event.

use std::{path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use toolkit_core::{
    bus::{Event, EventBus},
    events::Source,
    modules::Modules,
};
use utoipa::{OpenApi, ToSchema};

use crate::{
    application::{self, ClipError, ClipLookup, ClipsService, Entry, Playback},
    domain::Clip,
};

const CLIP_STATE: &str = "clip.state";
const CLIP_STATE_VERSION: u32 = 1;

/// Same shape as the host's admin error body: a stable `<area>.<reason>` code.
#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    code: String,
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(ErrorBody { code: code.into() })).into_response()
}

fn clip_error(e: ClipError) -> Response {
    let status = match &e {
        ClipError::InvalidInput | ClipError::InvalidBuffer => StatusCode::BAD_REQUEST,
        ClipError::NotFound => StatusCode::NOT_FOUND,
        ClipError::QueueFull => StatusCode::CONFLICT,
        ClipError::SaveFailed(io) => {
            tracing::error!(code = "settings.save_failed", error = %io, "clip settings not saved");
            StatusCode::INTERNAL_SERVER_ERROR
        }
        ClipError::Twitch { status, .. } => {
            StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY)
        }
    };
    error(status, &e.to_string())
}

/// The player's state. The playing states are estimates from the clip's length: the embed reports nothing back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackState {
    Idle,
    Loading,
    EstimatedPlaying,
    Transitioning,
    Stopped,
    Error,
}

impl From<application::State> for PlaybackState {
    fn from(s: application::State) -> Self {
        use application::State as S;
        match s {
            S::Idle => Self::Idle,
            S::Loading => Self::Loading,
            S::EstimatedPlaying => Self::EstimatedPlaying,
            S::Transitioning => Self::Transitioning,
            S::Stopped => Self::Stopped,
            S::Error => Self::Error,
        }
    }
}

/// A clip with its Twitch text. `title` and `broadcasterName` are Twitch text: render them as text.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClipInfo {
    /// The Twitch clip ID.
    pub id: String,
    pub title: String,
    pub broadcaster_name: String,
    pub duration_ms: u64,
}

impl From<Clip> for ClipInfo {
    fn from(c: Clip) -> Self {
        Self {
            id: c.id,
            title: c.title,
            broadcaster_name: c.broadcaster_name,
            duration_ms: c.duration_ms,
        }
    }
}

/// A queued clip.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueueEntry {
    /// Tells apart the same clip queued twice.
    pub entry: String,
    #[serde(flatten)]
    pub clip: ClipInfo,
}

impl From<Entry> for QueueEntry {
    fn from(e: Entry) -> Self {
        Self {
            entry: e.entry,
            clip: e.clip.into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClipQueue {
    /// Waiting clips in play order, at most 50; the current clip is not among them.
    pub queue: Vec<QueueEntry>,
    pub state: PlaybackState,
    /// New for every play; kept in `idle` and `stopped`; `null` only before the first clip.
    #[schema(required)]
    pub session_id: Option<String>,
    /// The current clip, or the last one when `stopped`; `null` when `idle`.
    #[schema(required)]
    pub current: Option<ClipInfo>,
    /// Estimated: counted from the start of `estimated_playing`, the clip's length in `transitioning`, else `null`.
    #[schema(required)]
    pub elapsed_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ClipInput {
    /// A Twitch clip URL (`clips.twitch.tv/<id>`, `clips.twitch.tv/embed?clip=<id>` or `twitch.tv/<channel>/clip/<id>`)
    /// or a clip ID.
    pub input: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionInput {
    /// The session the command is for; a session that is over makes the command do nothing.
    pub session_id: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClipSettings {
    /// Seconds between a clip's estimated end and the next clip, 0–30.
    pub buffer_secs: u32,
}

/// The `payload` of a `clip.state` event: what `/overlay/clip/` needs to show the embed and its caption.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClipStatePayload {
    #[schema(required)]
    pub session_id: Option<String>,
    pub state: PlaybackState,
    /// `null` when `idle`.
    #[schema(required)]
    pub clip: Option<ClipRef>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClipRef {
    /// The Twitch clip ID, for the embed URL.
    pub id: String,
    /// Twitch text: render it as text.
    pub title: String,
    /// Twitch text: render it as text.
    pub broadcaster_name: String,
    pub duration_ms: u64,
}

/// A `clip.state` event exactly as `/ws/overlay` sends it. The latest one is kept in snapshots, so a reloaded
/// overlay picks up the current session.
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClipStateEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: ClipStateType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: ClipStatePayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum ClipStateType {
    #[serde(rename = "clip.state")]
    ClipState,
}

fn state_payload(p: &Playback) -> ClipStatePayload {
    ClipStatePayload {
        session_id: p.session_id.clone(),
        state: p.state.into(),
        clip: p.clip.as_ref().map(|c| ClipRef {
            id: c.id.clone(),
            title: c.title.clone(),
            broadcaster_name: c.broadcaster_name.clone(),
            duration_ms: c.duration_ms,
        }),
    }
}

impl ClipsService {
    /// Publishes the player's state on `bus` as `clip.state`, keeping the latest one for snapshots.
    pub fn new(
        bus: Arc<EventBus>,
        modules: Arc<Modules>,
        data_dir: PathBuf,
        lookup: Arc<dyn ClipLookup>,
    ) -> Self {
        bus.retain(CLIP_STATE, 1);
        Self::with_publisher(lookup, modules, data_dir, move |p| {
            bus.publish(Event {
                id: uuid::Uuid::now_v7().to_string(),
                event_type: CLIP_STATE,
                version: CLIP_STATE_VERSION,
                occurred_at: chrono::Utc::now()
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                source: Source::System,
                payload: serde_json::to_value(state_payload(p)).expect("serializable"),
            });
        })
    }
}

/// The admin API.
pub fn routes(service: Arc<ClipsService>) -> Router {
    Router::new()
        .route(
            "/api/admin/clips",
            get(list_clips).post(add_clip).delete(clear_clips),
        )
        .route("/api/admin/clips/{entry}", delete(remove_clip))
        .route("/api/admin/clips/skip", post(skip_clip))
        .route("/api/admin/clips/stop", post(stop_clip))
        .route("/api/admin/clips/play", post(play_clip))
        .route("/api/admin/clips/replay", post(replay_clip))
        .route(
            "/api/admin/clips/settings",
            get(clip_settings).put(set_clip_settings),
        )
        .with_state(service)
}

/// The queue and the player's state.
#[utoipa::path(
    get,
    path = "/api/admin/clips",
    tag = "admin",
    responses((status = 200, body = ClipQueue))
)]
pub(crate) async fn list_clips(State(s): State<Arc<ClipsService>>) -> Json<ClipQueue> {
    let p = s.playback();
    Json(ClipQueue {
        queue: s.list().into_iter().map(Into::into).collect(),
        state: p.state.into(),
        session_id: p.session_id,
        current: p.clip.map(Into::into),
        elapsed_ms: p.elapsed_ms,
    })
}

/// Looks the clip up on Twitch (Helix Get Clips) and queues it at the end; an idle player starts it.
#[utoipa::path(
    post,
    path = "/api/admin/clips",
    tag = "admin",
    request_body = ClipInput,
    responses(
        (status = 200, body = QueueEntry),
        (status = 400, description = "`clip.invalid_input`: not a Twitch clip URL or ID; other `twitch.*` sign-in codes such as `twitch.sign_in_expired` or `twitch.invalid_response`", body = ErrorBody),
        (status = 404, description = "`clip.not_found`: Twitch has no such clip", body = ErrorBody),
        (status = 409, description = "`clip.queue_full` (50 clips), `twitch.client_id_required` or `twitch.sign_in_required`", body = ErrorBody),
        (status = 500, description = "`twitch.storage_failed`", body = ErrorBody),
        (status = 502, description = "`twitch.request_refused`, `twitch.rate_limited` or `twitch.unreachable`", body = ErrorBody),
    )
)]
pub(crate) async fn add_clip(
    State(s): State<Arc<ClipsService>>,
    body: Result<Json<ClipInput>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return clip_error(ClipError::InvalidInput);
    };
    match s.add(&b.input).await {
        Ok(entry) => Json(QueueEntry::from(entry)).into_response(),
        Err(e) => clip_error(e),
    }
}

/// Removes one queued clip; an entry that is not queued is ignored. The current clip is not in the queue.
#[utoipa::path(
    delete,
    path = "/api/admin/clips/{entry}",
    tag = "admin",
    params(("entry" = String, Path)),
    responses((status = 204))
)]
pub(crate) async fn remove_clip(
    State(s): State<Arc<ClipsService>>,
    Path(entry): Path<String>,
) -> StatusCode {
    s.remove(&entry);
    StatusCode::NO_CONTENT
}

/// Empties the queue; the current clip plays on.
#[utoipa::path(
    delete,
    path = "/api/admin/clips",
    tag = "admin",
    responses((status = 204))
)]
pub(crate) async fn clear_clips(State(s): State<Arc<ClipsService>>) -> StatusCode {
    s.clear();
    StatusCode::NO_CONTENT
}

fn session_command(
    body: Result<Json<SessionInput>, JsonRejection>,
    command: impl FnOnce(&str),
) -> Response {
    match body {
        Ok(Json(b)) => {
            command(&b.session_id);
            StatusCode::NO_CONTENT.into_response()
        }
        Err(_) => clip_error(ClipError::InvalidInput),
    }
}

/// Moves on to the next clip. Does nothing unless `sessionId` is the current, playing session, so a double click
/// skips once.
#[utoipa::path(
    post,
    path = "/api/admin/clips/skip",
    tag = "admin",
    request_body = SessionInput,
    responses((status = 204), (status = 400, description = "`clip.invalid_input`", body = ErrorBody))
)]
pub(crate) async fn skip_clip(
    State(s): State<Arc<ClipsService>>,
    body: Result<Json<SessionInput>, JsonRejection>,
) -> Response {
    session_command(body, |id| s.skip(id))
}

/// Plays the current or last clip again in a new session. Does nothing unless `sessionId` is the current session.
#[utoipa::path(
    post,
    path = "/api/admin/clips/replay",
    tag = "admin",
    request_body = SessionInput,
    responses((status = 204), (status = 400, description = "`clip.invalid_input`", body = ErrorBody))
)]
pub(crate) async fn replay_clip(
    State(s): State<Arc<ClipsService>>,
    body: Result<Json<SessionInput>, JsonRejection>,
) -> Response {
    session_command(body, |id| s.replay(id))
}

/// Stops the current clip; nothing more plays until play or replay.
#[utoipa::path(
    post,
    path = "/api/admin/clips/stop",
    tag = "admin",
    responses((status = 204))
)]
pub(crate) async fn stop_clip(State(s): State<Arc<ClipsService>>) -> StatusCode {
    s.stop();
    StatusCode::NO_CONTENT
}

/// Plays the next queued clip when stopped or idle; otherwise does nothing.
#[utoipa::path(
    post,
    path = "/api/admin/clips/play",
    tag = "admin",
    responses((status = 204))
)]
pub(crate) async fn play_clip(State(s): State<Arc<ClipsService>>) -> StatusCode {
    s.play();
    StatusCode::NO_CONTENT
}

#[utoipa::path(
    get,
    path = "/api/admin/clips/settings",
    tag = "admin",
    responses((status = 200, body = ClipSettings))
)]
pub(crate) async fn clip_settings(State(s): State<Arc<ClipsService>>) -> Json<ClipSettings> {
    Json(ClipSettings {
        buffer_secs: s.buffer_secs(),
    })
}

#[utoipa::path(
    put,
    path = "/api/admin/clips/settings",
    tag = "admin",
    request_body = ClipSettings,
    responses(
        (status = 204),
        (status = 400, description = "`clip.invalid_buffer`: not 0–30", body = ErrorBody),
        (status = 500, description = "`settings.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn set_clip_settings(
    State(s): State<Arc<ClipsService>>,
    body: Result<Json<ClipSettings>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return clip_error(ClipError::InvalidBuffer);
    };
    match s.set_buffer_secs(b.buffer_secs) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => clip_error(e),
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(
        list_clips,
        add_clip,
        remove_clip,
        clear_clips,
        skip_clip,
        replay_clip,
        stop_clip,
        play_clip,
        clip_settings,
        set_clip_settings
    ),
    components(schemas(ClipStateEvent))
)]
struct ClipsApi;

/// This module's part of the Host's OpenAPI document.
pub fn openapi() -> utoipa::openapi::OpenApi {
    ClipsApi::openapi()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{
        LookupError,
        tests::{FakeLookup, KNOWN, data_dir},
    };
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    fn app(name: &str, failure: Option<LookupError>) -> (Router, Arc<EventBus>) {
        let dir = data_dir(&format!("http-{name}"));
        let bus = Arc::new(EventBus::default());
        let service = ClipsService::new(
            bus.clone(),
            Arc::new(Modules::load(dir.clone())),
            dir,
            FakeLookup::new(failure),
        );
        (routes(Arc::new(service)), bus)
    }

    async fn send(app: &Router, req: Request<Body>) -> (StatusCode, serde_json::Value) {
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    fn json(method: &str, uri: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap()
    }

    fn empty(method: &str, uri: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap()
    }

    async fn settle() {
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }

    #[tokio::test(start_paused = true)]
    async fn add_list_remove_and_clear() {
        let (app, _) = app("queue", None);
        let url = format!(r#"{{"input":"https://www.twitch.tv/twitch/clip/{KNOWN}"}}"#);
        let (status, added) = send(&app, json("POST", "/api/admin/clips", &url)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(added["id"], KNOWN);
        assert_eq!(added["broadcasterName"], "Twitch");
        assert_eq!(added["durationMs"], 30_000);
        settle().await;

        let (_, second) = send(&app, json("POST", "/api/admin/clips", &url)).await;
        send(&app, json("POST", "/api/admin/clips", &url)).await;
        let (_, list) = send(&app, empty("GET", "/api/admin/clips")).await;
        assert_eq!(list["state"], "loading");
        assert_eq!(list["current"]["id"], KNOWN);
        assert!(list["sessionId"].is_string());
        assert_eq!(list["elapsedMs"], serde_json::Value::Null);
        assert_eq!(list["queue"].as_array().unwrap().len(), 2);
        assert_eq!(list["queue"][0], second);

        let entry = second["entry"].as_str().unwrap();
        for uri in [
            format!("/api/admin/clips/{entry}"),
            "/api/admin/clips/unknown".into(),
        ] {
            assert_eq!(
                send(&app, empty("DELETE", &uri)).await.0,
                StatusCode::NO_CONTENT
            );
        }
        let (_, list) = send(&app, empty("GET", "/api/admin/clips")).await;
        assert_eq!(list["queue"].as_array().unwrap().len(), 1);

        let clear = empty("DELETE", "/api/admin/clips");
        assert_eq!(send(&app, clear).await.0, StatusCode::NO_CONTENT);
        let (_, list) = send(&app, empty("GET", "/api/admin/clips")).await;
        assert_eq!(list["queue"], serde_json::json!([]));
        assert_eq!(list["state"], "loading", "clearing keeps the current clip");
    }

    #[tokio::test(start_paused = true)]
    async fn commands_follow_the_session_and_publish_the_state() {
        let (app, bus) = app("commands", None);
        let (snapshot, mut events) = bus.subscribe();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
        assert_eq!(
            snapshot["payload"]["clip.state"][0]["payload"]["state"],
            "idle"
        );

        let url = format!(r#"{{"input":"{KNOWN}"}}"#);
        send(&app, json("POST", "/api/admin/clips", &url)).await;
        send(&app, json("POST", "/api/admin/clips", &url)).await;
        settle().await;
        let loading: serde_json::Value =
            serde_json::from_str(&events.recv().await.unwrap()).unwrap();
        assert_eq!(loading["type"], "clip.state");
        assert_eq!(loading["source"], "system");
        let payload = &loading["payload"];
        assert_eq!(payload["state"], "loading");
        assert_eq!(
            payload["clip"],
            serde_json::json!({ "id": KNOWN, "title": "F1 2017 E3 Gameplay!",
                                "broadcasterName": "Twitch", "durationMs": 30_000 })
        );
        let session = payload["sessionId"].as_str().unwrap().to_owned();

        let skip = format!(r#"{{"sessionId":"{session}"}}"#);
        for _ in 0..2 {
            let res = send(&app, json("POST", "/api/admin/clips/skip", &skip)).await;
            assert_eq!(res.0, StatusCode::NO_CONTENT);
        }
        settle().await;
        let (_, list) = send(&app, empty("GET", "/api/admin/clips")).await;
        assert_eq!(list["queue"], serde_json::json!([]), "skipped once");
        assert_ne!(list["sessionId"], session.as_str());

        assert_eq!(
            send(&app, empty("POST", "/api/admin/clips/stop")).await.0,
            StatusCode::NO_CONTENT
        );
        settle().await;
        assert_eq!(
            send(&app, empty("GET", "/api/admin/clips")).await.1["state"],
            "stopped"
        );
        assert_eq!(
            send(&app, empty("POST", "/api/admin/clips/play")).await.0,
            StatusCode::NO_CONTENT
        );
        let snapshot = bus.subscribe().0;
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
        let kept = snapshot["payload"]["clip.state"].as_array().unwrap();
        assert_eq!(kept.len(), 1, "only the latest state is kept");
        assert_eq!(kept[0]["payload"]["state"], "stopped");

        let bad = send(&app, json("POST", "/api/admin/clips/replay", "{}")).await;
        assert_eq!(
            bad,
            (
                StatusCode::BAD_REQUEST,
                serde_json::json!({ "code": "clip.invalid_input" })
            )
        );
    }

    #[tokio::test]
    async fn the_buffer_setting_round_trips() {
        let (app, _) = app("settings", None);
        let get = || empty("GET", "/api/admin/clips/settings");
        assert_eq!(
            send(&app, get()).await.1,
            serde_json::json!({ "bufferSecs": 2 })
        );
        let put = |body: &str| json("PUT", "/api/admin/clips/settings", body);
        assert_eq!(
            send(&app, put(r#"{"bufferSecs":5}"#)).await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(send(&app, get()).await.1["bufferSecs"], 5);
        for body in [r#"{"bufferSecs":31}"#, r#"{"bufferSecs":-1}"#, "{}"] {
            let res = send(&app, put(body)).await;
            assert_eq!(
                res,
                (
                    StatusCode::BAD_REQUEST,
                    serde_json::json!({ "code": "clip.invalid_buffer" })
                )
            );
        }
    }

    #[tokio::test]
    async fn refusals_carry_their_codes() {
        let (app, _) = app("refusals", None);
        for (body, status, code) in [
            (
                r#"{"input":"https://example.com/clip"}"#,
                StatusCode::BAD_REQUEST,
                "clip.invalid_input",
            ),
            ("not json", StatusCode::BAD_REQUEST, "clip.invalid_input"),
            (
                r#"{"input":"GoneClip"}"#,
                StatusCode::NOT_FOUND,
                "clip.not_found",
            ),
        ] {
            let res = send(&app, json("POST", "/api/admin/clips", body)).await;
            assert_eq!(res, (status, serde_json::json!({ "code": code })));
        }

        let (signed_out, _) = self::app(
            "refusals-out",
            Some(LookupError::Twitch {
                status: 409,
                code: "twitch.sign_in_required".into(),
            }),
        );
        let body = format!(r#"{{"input":"{KNOWN}"}}"#);
        let (status, body) = send(&signed_out, json("POST", "/api/admin/clips", &body)).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["code"], "twitch.sign_in_required");
    }
}
