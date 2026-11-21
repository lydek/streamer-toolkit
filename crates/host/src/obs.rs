//! OBS connection: runs the `integrations` adapter, publishes its status as `obs.status` and the streaming-software
//! state as `obs.state` (both kept in the WebSocket snapshot), and lets the admin API change host, port and
//! password. Admin routes; the caller adds the token check.

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, put},
};
use integrations::{
    obs::{Obs, ObsControl, ObsState, ObsStatus},
    secrets::SecretStore,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tokio::{sync::watch, task::AbortHandle};
use toolkit_core::{
    bus::{Event, EventBus},
    events::Source,
    settings,
    streaming::{StreamingError, StreamingSoftware, StreamingState, StreamingStats},
};
use utoipa::ToSchema;

use crate::admin::{ErrorBody, error};

pub(crate) const STATUS_EVENT: &str = "obs.status";
const STATUS_VERSION: u32 = 1;
pub(crate) const STATE_EVENT: &str = "obs.state";
const STATE_VERSION: u32 = 1;

/// The `payload` of an `obs.status` event, and the `status` of `GET /api/admin/obs`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub(crate) struct ObsStatusPayload {
    state: ObsState,
    /// Stable code such as `obs.auth_failed`; `null` when connected or right after startup.
    #[schema(required)]
    reason: Option<String>,
}

impl From<ObsStatus> for ObsStatusPayload {
    fn from(status: ObsStatus) -> Self {
        Self {
            state: status.state,
            reason: status.reason.map(Into::into),
        }
    }
}

/// An `obs.status` event exactly as `/ws/overlay` sends it; the snapshot keeps only the latest one.
// The bus serializes its own envelope; the snapshot test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObsStatusEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: ObsStatusType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: ObsStatusPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum ObsStatusType {
    #[serde(rename = "obs.status")]
    ObsStatus,
}

/// An `obs.state` event exactly as `/ws/overlay` sends it; the snapshot keeps only the latest one.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObsStateEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: ObsStateType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: StreamingState,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum ObsStateType {
    #[serde(rename = "obs.state")]
    ObsState,
}

/// Keeps the latest `obs.status` and `obs.state` for snapshots.
pub(crate) fn retain(bus: &EventBus) {
    bus.retain(STATUS_EVENT, 1);
    bus.retain(STATE_EVENT, 1);
}

pub(crate) fn publish_state(bus: &EventBus, state: &StreamingState) {
    bus.publish(Event {
        id: uuid::Uuid::now_v7().to_string(),
        event_type: STATE_EVENT,
        version: STATE_VERSION,
        occurred_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        source: Source::Obs,
        payload: serde_json::to_value(state).expect("state serializes"),
    });
}

pub(crate) fn publish(bus: &EventBus, status: ObsStatus) {
    bus.publish(Event {
        id: uuid::Uuid::now_v7().to_string(),
        event_type: STATUS_EVENT,
        version: STATUS_VERSION,
        occurred_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        source: Source::Obs,
        payload: serde_json::to_value(ObsStatusPayload::from(status)).expect("status serializes"),
    });
}

pub(crate) struct ObsLink {
    data_dir: PathBuf,
    secrets: Arc<dyn SecretStore>,
    status: Arc<watch::Sender<ObsStatus>>,
    state: Arc<watch::Sender<StreamingState>>,
    /// The current connection's number; a replaced one stops writing (see [`Obs::generation`]).
    generation: Arc<AtomicU64>,
    /// The current connection's client, for commands.
    control: Arc<ObsControl>,
    task: Mutex<Option<AbortHandle>>,
}

impl ObsLink {
    /// Connects with the saved settings and forwards every status and state change to the bus.
    pub(crate) fn start(
        data_dir: PathBuf,
        secrets: Arc<dyn SecretStore>,
        bus: Arc<EventBus>,
    ) -> Arc<Self> {
        let status = Arc::new(watch::channel(ObsStatus::CONNECTING).0);
        retain(&bus);
        let mut changes = status.subscribe();
        let status_bus = bus.clone();
        tokio::spawn(async move {
            loop {
                let current = *changes.borrow_and_update();
                publish(&status_bus, current);
                if changes.changed().await.is_err() {
                    return;
                }
            }
        });
        let state = Arc::new(watch::channel(StreamingState::default()).0);
        let mut state_changes = state.subscribe();
        tokio::spawn(async move {
            loop {
                let current = state_changes.borrow_and_update().clone();
                publish_state(&bus, &current);
                if state_changes.changed().await.is_err() {
                    return;
                }
            }
        });
        let link = Arc::new(Self {
            data_dir,
            secrets,
            status,
            state,
            generation: Arc::new(AtomicU64::new(0)),
            control: Arc::default(),
            task: Mutex::new(None),
        });
        link.restart();
        link
    }

    /// Follows the streaming state, the scene on air included.
    pub(crate) fn states(&self) -> watch::Receiver<StreamingState> {
        self.state.subscribe()
    }

    /// Starts a connection with the saved settings and stops the previous one.
    fn restart(&self) {
        // Held while reading settings too, so concurrent restarts end with the task for the latest settings.
        let mut current = self.task.lock().unwrap();
        let saved = settings::load(&self.data_dir).obs;
        // The old connection stops writing first, then its scenes and outputs go: they belong to the old settings.
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(old) = current.take() {
            old.abort();
        }
        self.control.clear();
        self.state.send_replace(StreamingState::default());
        let task = tokio::spawn(
            Obs {
                host: saved.host,
                port: saved.port,
                secrets: self.secrets.clone(),
                status: self.status.clone(),
                state: self.state.clone(),
                generation,
                current: self.generation.clone(),
                control: self.control.clone(),
            }
            .run(),
        );
        *current = Some(task.abort_handle());
    }
}

impl ObsLink {
    /// `NotFound` for a scene (or an item of it) the latest state does not list; OBS answers the same, but this
    /// gives the same answer without a round trip and before a client exists.
    fn check(&self, scene: &str, item: Option<i64>) -> Result<(), StreamingError> {
        let state = self.state.borrow();
        if state.scene.is_none() {
            return Err(StreamingError::NotConnected);
        }
        let found = state.scenes.iter().find(|s| s.name == scene);
        match (found, item) {
            (Some(s), Some(id)) if s.items.iter().any(|i| i.id == id) => Ok(()),
            (Some(_), None) => Ok(()),
            _ => Err(StreamingError::NotFound),
        }
    }
}

impl StreamingSoftware for ObsLink {
    fn state(&self) -> StreamingState {
        self.state.borrow().clone()
    }

    async fn switch_scene(&self, scene: &str) -> Result<(), StreamingError> {
        self.check(scene, None)?;
        self.control.switch_scene(scene).await
    }

    async fn set_item_enabled(
        &self,
        scene: &str,
        item: i64,
        enabled: bool,
    ) -> Result<(), StreamingError> {
        self.check(scene, Some(item))?;
        self.control.set_item_enabled(scene, item, enabled).await
    }

    async fn stats(&self) -> Result<StreamingStats, StreamingError> {
        self.control.stats().await
    }

    async fn set_item_locked(
        &self,
        scene: &str,
        item: i64,
        locked: bool,
    ) -> Result<(), StreamingError> {
        self.check(scene, Some(item))?;
        self.control.set_item_locked(scene, item, locked).await
    }
}

impl Drop for ObsLink {
    /// A dropped router (e.g. a failed port retry) must not leave a connection retrying in the background.
    fn drop(&mut self) {
        if let Some(task) = self.task.lock().unwrap().take() {
            task.abort();
        }
    }
}

pub(crate) fn routes(link: Arc<ObsLink>) -> Router {
    Router::new()
        .route("/api/admin/obs", get(obs_overview).put(update_obs))
        .route("/api/admin/obs/scene", put(switch_scene))
        .route("/api/admin/obs/item", put(update_item))
        .with_state(link)
}

fn command_response(result: Result<(), StreamingError>) -> Response {
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(StreamingError::NotConnected) => error(StatusCode::CONFLICT, "obs.not_connected"),
        Err(StreamingError::NotFound) => error(StatusCode::NOT_FOUND, "obs.not_found"),
        Err(StreamingError::Failed) => error(StatusCode::BAD_GATEWAY, "obs.request_failed"),
    }
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct SceneBody {
    scene: String,
}

/// Puts a scene on air; `obs.state` shows the switch once OBS has made it.
#[utoipa::path(
    put,
    path = "/api/admin/obs/scene",
    tag = "admin",
    request_body = SceneBody,
    responses(
        (status = 204),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(crate) async fn switch_scene(
    State(link): State<Arc<ObsLink>>,
    Json(body): Json<SceneBody>,
) -> Response {
    command_response(link.switch_scene(&body.scene).await)
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct ItemBody {
    scene: String,
    /// The item's `id` in `obs.state`.
    item: i64,
    /// Shows or hides the item; absent leaves it.
    #[serde(default)]
    enabled: Option<bool>,
    /// Locks or unlocks the item; absent leaves it.
    #[serde(default)]
    locked: Option<bool>,
}

/// Shows, hides, locks or unlocks a scene item; `obs.state` shows the change once OBS has made it.
#[utoipa::path(
    put,
    path = "/api/admin/obs/item",
    tag = "admin",
    request_body = ItemBody,
    responses(
        (status = 204),
        (status = 400, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(crate) async fn update_item(
    State(link): State<Arc<ObsLink>>,
    Json(body): Json<ItemBody>,
) -> Response {
    if body.enabled.is_none() && body.locked.is_none() {
        return error(StatusCode::BAD_REQUEST, "obs.nothing_to_change");
    }
    if let Some(enabled) = body.enabled
        && let Err(e) = link.set_item_enabled(&body.scene, body.item, enabled).await
    {
        return command_response(Err(e));
    }
    if let Some(locked) = body.locked {
        return command_response(link.set_item_locked(&body.scene, body.item, locked).await);
    }
    command_response(Ok(()))
}

#[derive(Serialize, ToSchema)]
pub(crate) struct ObsOverview {
    host: String,
    port: u16,
    /// Whether a password is stored; the password itself is never returned.
    password_set: bool,
    status: ObsStatusPayload,
}

#[utoipa::path(
    get,
    path = "/api/admin/obs",
    tag = "admin",
    responses(
        (status = 200, body = ObsOverview),
        (status = 500, body = ErrorBody),
    )
)]
pub(crate) async fn obs_overview(State(link): State<Arc<ObsLink>>) -> Response {
    let Ok(password) = link.secrets.read().map(|p| p.map(SecretString::from)) else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "obs.secret_store_failed");
    };
    let saved = settings::load(&link.data_dir).obs;
    let status = (*link.status.borrow()).into();
    Json(ObsOverview {
        host: saved.host,
        port: saved.port,
        password_set: password.is_some(),
        status,
    })
    .into_response()
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct ObsSettingsBody {
    /// Host name or IPv4 address.
    host: String,
    /// 1-65535.
    port: u16,
    /// Absent keeps the stored password; empty removes it (OBS with authentication off).
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    password: Option<SecretString>,
}

/// Host names and IPv4 addresses only, so the value is safe inside `ws://<host>:<port>`.
fn valid_host(host: &str) -> bool {
    (1..=253).contains(&host.len())
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
}

/// Saves the address (and the password, when given) and reconnects with them.
#[utoipa::path(
    put,
    path = "/api/admin/obs",
    tag = "admin",
    request_body = ObsSettingsBody,
    responses(
        (status = 204),
        (status = 400, body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
pub(crate) async fn update_obs(
    State(link): State<Arc<ObsLink>>,
    Json(body): Json<ObsSettingsBody>,
) -> Response {
    let host = body.host.trim().to_owned();
    if !valid_host(&host) || body.port == 0 {
        return error(StatusCode::BAD_REQUEST, "obs.invalid_address");
    }
    if settings::update(&link.data_dir, |s| {
        s.obs.host = host;
        s.obs.port = body.port;
    })
    .is_err()
    {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed");
    }
    // After the settings, so a failed save never leaves a new password silently in use.
    let password_saved =
        body.password
            .as_ref()
            .is_none_or(|password| match password.expose_secret() {
                "" => link.secrets.delete().is_ok(),
                value => link.secrets.write(value).is_ok(),
            });
    // The saved address applies either way; the response says whether the password did.
    link.restart();
    if !password_saved {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "obs.secret_store_failed");
    }
    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use integrations::secrets::SecretStoreError;
    use std::time::Duration;

    struct NoPassword;
    impl SecretStore for NoPassword {
        fn read(&self) -> Result<Option<String>, SecretStoreError> {
            Ok(None)
        }
        fn write(&self, _: &str) -> Result<(), SecretStoreError> {
            Ok(())
        }
        fn delete(&self) -> Result<(), SecretStoreError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn status_reaches_the_websocket_snapshot() {
        let dir = std::env::temp_dir().join(format!("st-obs-snapshot-{}", std::process::id()));
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        settings::update(&dir, |s| s.obs.port = port).unwrap();
        let bus = Arc::new(EventBus::default());
        let _link = ObsLink::start(dir.clone(), Arc::new(NoPassword), bus.clone());

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let (snapshot, _) = bus.subscribe();
            let snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
            let status = &snapshot["payload"][STATUS_EVENT][0];
            if status["payload"]["reason"] == "obs.unreachable" {
                assert_eq!(status["payload"]["state"], "reconnecting");
                assert_eq!(status["source"], "obs");
                assert_eq!(
                    snapshot["payload"][STATUS_EVENT].as_array().unwrap().len(),
                    1
                );
                break;
            }
            assert!(tokio::time::Instant::now() < deadline, "{snapshot}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn host_validation() {
        assert!(valid_host("127.0.0.1"));
        assert!(valid_host("obs-pc.local"));
        assert!(!valid_host(""));
        assert!(!valid_host("evil.example/path"));
        assert!(!valid_host("user@host"));
        assert!(!valid_host("host:1234"));
        assert!(!valid_host(&"a".repeat(254)));
    }
}
