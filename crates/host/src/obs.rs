//! OBS connection: runs the `integrations` adapter, publishes its status as `obs.status` and the streaming-software
//! state as `obs.state` (both kept in the WebSocket snapshot), and lets the admin API change host, port and
//! password, and lists the browser sources that show this Host's overlays. Admin routes; the caller adds the token
//! check.

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use axum::{
    Json, Router,
    extract::Query,
    extract::State,
    http::{StatusCode, Uri},
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
    settings::{self, ExternalService},
    streaming::{
        BrowserSource, Canvas, InputMeter, ItemSize, ItemTransform, Monitor, Placement,
        StreamingError, StreamingSoftware, StreamingState, StreamingStats, TransformChange,
    },
};
use utoipa::ToSchema;

use crate::admin::{ErrorBody, error};

pub(crate) const STATUS_EVENT: &str = "obs.status";
const STATUS_VERSION: u32 = 1;
pub(crate) const STATE_EVENT: &str = "obs.state";
/// The shortest time between two `obs.state` events.
const STATE_GAP: std::time::Duration = std::time::Duration::from_millis(150);
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
    /// The port this Host serves on: a browser source shows one of its overlays when its address uses it.
    /// (Read by the source settings routes too.)
    port: u16,
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
        port: u16,
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
                // A fader dragged in OBS changes the state many times a second: the latest goes out at most every
                // STATE_GAP, the first after a quiet spell at once.
                tokio::time::sleep(STATE_GAP).await;
                if state_changes.changed().await.is_err() {
                    return;
                }
            }
        });
        let link = Arc::new(Self {
            data_dir,
            port,
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

    /// The port this Host serves on (its overlays' addresses use it).
    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    /// Follows the streaming state, the scene on air included.
    pub(crate) fn states(&self) -> watch::Receiver<StreamingState> {
        self.state.subscribe()
    }

    /// Starts a connection with the saved settings and stops the previous one; none while OBS is not used.
    pub(crate) fn restart(&self) {
        // Held while reading settings too, so concurrent restarts end with the task for the latest settings.
        let mut current = self.task.lock().unwrap();
        let all = settings::load(&self.data_dir);
        let saved = all.obs.clone();
        // The old connection stops writing first, then its scenes and outputs go: they belong to the old settings.
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(old) = current.take() {
            old.abort();
        }
        self.control.clear();
        self.state.send_replace(StreamingState::default());
        if all.unused(ExternalService::Obs) {
            // Not "connected" from before: no connection, and none tried.
            self.status.send_replace(ObsStatus::UNUSED);
            return;
        }
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

    async fn browser_sources(&self) -> Result<Vec<BrowserSource>, StreamingError> {
        if self.state.borrow().scene.is_none() {
            return Err(StreamingError::NotConnected);
        }
        self.control.browser_sources().await
    }

    async fn create_browser_source(
        &self,
        scene: &str,
        name: &str,
        url: &str,
        size: (u32, u32),
    ) -> Result<(), StreamingError> {
        self.check(scene, None)?;
        self.control
            .create_browser_source(scene, name, url, size)
            .await
    }

    async fn add_source(&self, scene: &str, source: &str) -> Result<(), StreamingError> {
        self.check(scene, None)?;
        self.control.add_source(scene, source).await
    }

    async fn set_item_position(
        &self,
        scene: &str,
        item: i64,
        position: u32,
    ) -> Result<(), StreamingError> {
        self.check(scene, Some(item))?;
        let count = self
            .state
            .borrow()
            .scenes
            .iter()
            .find(|s| s.name == scene)
            .map_or(0, |s| s.items.len());
        if position as usize >= count {
            return Err(StreamingError::NotFound);
        }
        self.control.set_item_position(scene, item, position).await
    }

    async fn placements(
        &self,
        scene: &str,
    ) -> Result<(Canvas, Vec<(i64, Placement, ItemSize)>), StreamingError> {
        self.check(scene, None)?;
        self.control.placements(scene).await
    }

    async fn item_transform(
        &self,
        scene: &str,
        item: i64,
    ) -> Result<ItemTransform, StreamingError> {
        self.check(scene, Some(item))?;
        self.control.item_transform(scene, item).await
    }

    async fn set_item_transform(
        &self,
        scene: &str,
        item: i64,
        change: TransformChange,
    ) -> Result<(), StreamingError> {
        self.check(scene, Some(item))?;
        self.control.set_item_transform(scene, item, change).await
    }

    async fn input_settings(
        &self,
        input: &str,
    ) -> Result<(String, serde_json::Map<String, serde_json::Value>), StreamingError> {
        self.control.input_settings(input).await
    }

    async fn set_input_settings(
        &self,
        input: &str,
        settings: serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), StreamingError> {
        self.control.set_input_settings(input, settings).await
    }

    async fn press_input_button(&self, input: &str, property: &str) -> Result<(), StreamingError> {
        self.control.press_input_button(input, property).await
    }

    async fn input_list_items(
        &self,
        input: &str,
        property: &str,
    ) -> Result<Vec<(String, serde_json::Value)>, StreamingError> {
        self.control.input_list_items(input, property).await
    }

    fn input_meters(&self) -> Vec<InputMeter> {
        self.control.meters()
    }

    async fn set_input_volume(&self, input: &str, db: f32) -> Result<(), StreamingError> {
        self.control.set_input_volume(input, db).await
    }

    async fn set_input_mute(&self, input: &str, muted: bool) -> Result<(), StreamingError> {
        self.control.set_input_mute(input, muted).await
    }

    async fn set_input_monitor(&self, input: &str, monitor: Monitor) -> Result<(), StreamingError> {
        self.control.set_input_monitor(input, monitor).await
    }

    async fn set_item_size(
        &self,
        scene: &str,
        item: i64,
        size: ItemSize,
    ) -> Result<(), StreamingError> {
        self.check(scene, Some(item))?;
        self.control.set_item_size(scene, item, size).await
    }

    async fn set_placement(
        &self,
        scene: &str,
        item: i64,
        placement: Placement,
    ) -> Result<(), StreamingError> {
        self.check(scene, Some(item))?;
        self.control.set_placement(scene, item, placement).await
    }

    async fn set_studio_mode(&self, enabled: bool) -> Result<(), StreamingError> {
        if self.state.borrow().scene.is_none() {
            return Err(StreamingError::NotConnected);
        }
        self.control.set_studio_mode(enabled).await
    }

    async fn set_preview(&self, scene: &str) -> Result<(), StreamingError> {
        self.check(scene, None)?;
        self.control.set_preview(scene).await
    }

    async fn transition(&self, transition: Option<&str>) -> Result<(), StreamingError> {
        {
            let state = self.state.borrow();
            if state.scene.is_none() {
                return Err(StreamingError::NotConnected);
            }
            if transition.is_some_and(|t| !state.transitions.iter().any(|known| known == t)) {
                return Err(StreamingError::NotFound);
            }
        }
        self.control.transition(transition).await
    }

    async fn remove_item(&self, scene: &str, item: i64) -> Result<(), StreamingError> {
        self.check(scene, Some(item))?;
        self.control.remove_item(scene, item).await
    }
}

impl ObsLink {
    /// Whether OBS runs on this computer, where the Host's `127.0.0.1` addresses open.
    fn obs_is_local(&self) -> bool {
        matches!(
            settings::load(&self.data_dir).obs.host.as_str(),
            "127.0.0.1" | "localhost"
        )
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
        .route("/api/admin/obs/studio", put(set_studio))
        .route("/api/admin/obs/preview", put(set_preview_scene))
        .route(
            "/api/admin/obs/transition",
            axum::routing::post(transition_now),
        )
        .route("/api/admin/obs/item", put(update_item))
        .route("/api/admin/obs/placements", get(scene_placements))
        .route("/api/admin/obs/transform", get(item_transform))
        .route("/api/admin/obs/input", put(update_input))
        .route("/api/admin/obs/meters", get(input_meters))
        .route(
            "/api/admin/obs/sources",
            get(overlay_sources)
                .post(add_overlay)
                .delete(remove_overlay),
        )
        .with_state(link)
}

pub(crate) fn command_response(result: Result<(), StreamingError>) -> Response {
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
pub(crate) struct StudioBody {
    enabled: bool,
}

/// Turns OBS's studio mode on or off; `obs.state` follows once OBS has done it, also when it is changed in OBS.
#[utoipa::path(
    put,
    path = "/api/admin/obs/studio",
    tag = "admin",
    request_body = StudioBody,
    responses(
        (status = 204),
        (status = 409, body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(crate) async fn set_studio(
    State(link): State<Arc<ObsLink>>,
    Json(body): Json<StudioBody>,
) -> Response {
    command_response(link.set_studio_mode(body.enabled).await)
}

/// Puts a scene in the preview while studio mode is on; `obs.state`'s `preview` shows it.
#[utoipa::path(
    put,
    path = "/api/admin/obs/preview",
    tag = "admin",
    request_body = SceneBody,
    responses(
        (status = 204),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(crate) async fn set_preview_scene(
    State(link): State<Arc<ObsLink>>,
    Json(body): Json<SceneBody>,
) -> Response {
    command_response(link.set_preview(&body.scene).await)
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct TransitionBody {
    /// One of `obs.state`'s `transitions`; absent keeps the one in use.
    #[serde(default)]
    transition: Option<String>,
}

/// Puts the preview on air (studio mode), with the transition given or the one in use.
#[utoipa::path(
    post,
    path = "/api/admin/obs/transition",
    tag = "admin",
    request_body = TransitionBody,
    responses(
        (status = 204),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(crate) async fn transition_now(
    State(link): State<Arc<ObsLink>>,
    Json(body): Json<TransitionBody>,
) -> Response {
    command_response(link.transition(body.transition.as_deref()).await)
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
    /// Moves the item to this place in the scene, counted front to back as `obs.state` lists items (0 is the
    /// front); absent leaves it.
    #[serde(default)]
    position: Option<u32>,
    /// Puts the item at this canvas position and alignment; absent leaves it. Scale, rotation and crop stay.
    #[serde(default)]
    placement: Option<Placement>,
    /// Scales the item to this size in canvas pixels, each side 1-100000; absent leaves it. Position stays.
    #[serde(default)]
    size: Option<ItemSize>,
    /// Turns, flips, crops or fits the item (only the parts given; rotation -360-360°, crop 0-100000 px a side);
    /// absent leaves it. Position and size stay.
    #[serde(default)]
    transform: Option<TransformChange>,
}

/// Shows, hides, locks, unlocks or moves a scene item; `obs.state` shows the change once OBS has made it.
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
    if body.enabled.is_none()
        && body.locked.is_none()
        && body.position.is_none()
        && body.placement.is_none()
        && body.size.is_none()
        && body.transform.is_none()
    {
        return error(StatusCode::BAD_REQUEST, "obs.nothing_to_change");
    }
    let valid = |side: f32| side.is_finite() && (1.0..=100_000.0).contains(&side);
    if let Some(size) = body.size
        && !(valid(size.width) && valid(size.height))
    {
        return error(StatusCode::BAD_REQUEST, "obs.invalid_size");
    }
    if let Some(change) = body.transform {
        let turn_ok = change
            .rotation
            .is_none_or(|r| r.is_finite() && (-360.0..=360.0).contains(&r));
        let crop_ok = change.crop.is_none_or(|c| {
            [c.top, c.right, c.bottom, c.left]
                .iter()
                .all(|&n| n <= 100_000)
        });
        if !turn_ok || !crop_ok {
            return error(StatusCode::BAD_REQUEST, "obs.invalid_transform");
        }
    }
    if let Some(enabled) = body.enabled
        && let Err(e) = link.set_item_enabled(&body.scene, body.item, enabled).await
    {
        return command_response(Err(e));
    }
    if let Some(locked) = body.locked
        && let Err(e) = link.set_item_locked(&body.scene, body.item, locked).await
    {
        return command_response(Err(e));
    }
    if let Some(placement) = body.placement
        && let Err(e) = link.set_placement(&body.scene, body.item, placement).await
    {
        return command_response(Err(e));
    }
    if let Some(size) = body.size
        && let Err(e) = link.set_item_size(&body.scene, body.item, size).await
    {
        return command_response(Err(e));
    }
    if let Some(change) = body.transform
        && let Err(e) = link
            .set_item_transform(&body.scene, body.item, change)
            .await
    {
        return command_response(Err(e));
    }
    if let Some(position) = body.position {
        return command_response(
            link.set_item_position(&body.scene, body.item, position)
                .await,
        );
    }
    command_response(Ok(()))
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct PlacementsQuery {
    scene: String,
}

/// The inputs' latest audio levels (OBS's volume meters), for the mixer to poll while it shows; empty while OBS is
/// not connected.
#[utoipa::path(
    get,
    path = "/api/admin/obs/meters",
    tag = "admin",
    responses((status = 200, body = [InputMeter]))
)]
pub(crate) async fn input_meters(State(link): State<Arc<ObsLink>>) -> Json<Vec<InputMeter>> {
    Json(link.input_meters())
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct InputBody {
    /// An input of `obs.state`'s `inputs`.
    input: String,
    /// The fader in dB, -100 to 26; absent leaves it.
    #[serde(default)]
    volume_db: Option<f32>,
    #[serde(default)]
    muted: Option<bool>,
    #[serde(default)]
    monitor: Option<Monitor>,
}

/// Changes an input's fader, mute or monitoring (they belong to the input, in every scene); `obs.state` shows the
/// change once OBS has made it.
#[utoipa::path(
    put,
    path = "/api/admin/obs/input",
    tag = "admin",
    request_body = InputBody,
    responses(
        (status = 204),
        (status = 400, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(crate) async fn update_input(
    State(link): State<Arc<ObsLink>>,
    Json(body): Json<InputBody>,
) -> Response {
    if body.volume_db.is_none() && body.muted.is_none() && body.monitor.is_none() {
        return error(StatusCode::BAD_REQUEST, "obs.nothing_to_change");
    }
    if body
        .volume_db
        .is_some_and(|db| !db.is_finite() || !(-100.0..=26.0).contains(&db))
    {
        return error(StatusCode::BAD_REQUEST, "obs.invalid_volume");
    }
    if let Some(db) = body.volume_db
        && let Err(e) = link.set_input_volume(&body.input, db).await
    {
        return command_response(Err(e));
    }
    if let Some(muted) = body.muted
        && let Err(e) = link.set_input_mute(&body.input, muted).await
    {
        return command_response(Err(e));
    }
    if let Some(monitor) = body.monitor {
        return command_response(link.set_input_monitor(&body.input, monitor).await);
    }
    command_response(Ok(()))
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ItemQuery {
    scene: String,
    item: i64,
}

/// How an item is turned, flipped, cropped and fitted (asked when its 變形 tab opens).
#[utoipa::path(
    get,
    path = "/api/admin/obs/transform",
    tag = "admin",
    params(ItemQuery),
    responses(
        (status = 200, body = ItemTransform),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(crate) async fn item_transform(
    State(link): State<Arc<ObsLink>>,
    Query(query): Query<ItemQuery>,
) -> Response {
    match link.item_transform(&query.scene, query.item).await {
        Ok(t) => Json(t).into_response(),
        Err(e) => command_response(Err(e)),
    }
}

#[derive(Serialize, ToSchema)]
pub(crate) struct ScenePlacements {
    canvas: Canvas,
    items: Vec<ItemPlacement>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct ItemPlacement {
    /// The item's `id` in `obs.state`.
    item: i64,
    #[serde(flatten)]
    placement: Placement,
    #[serde(flatten)]
    size: ItemSize,
}

/// The canvas size and where each item of a scene sits (asked when the scene page shows the scene).
#[utoipa::path(
    get,
    path = "/api/admin/obs/placements",
    tag = "admin",
    params(PlacementsQuery),
    responses(
        (status = 200, body = ScenePlacements),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(crate) async fn scene_placements(
    State(link): State<Arc<ObsLink>>,
    Query(query): Query<PlacementsQuery>,
) -> Response {
    match link.placements(&query.scene).await {
        Ok((canvas, items)) => Json(ScenePlacements {
            canvas,
            items: items
                .into_iter()
                .map(|(item, placement, size)| ItemPlacement {
                    item,
                    placement,
                    size,
                })
                .collect(),
        })
        .into_response(),
        Err(e) => command_response(Err(e)),
    }
}

/// The overlay path, with its query, that `url` opens on this Host (port `port`); `None` for any other address.
/// Other addresses go no further than this: they may hold another service's token.
pub(crate) fn own_overlay(url: &str, port: u16) -> Option<String> {
    let uri: Uri = url.parse().ok()?;
    let ours = uri.scheme_str() == Some("http")
        && matches!(uri.host(), Some("127.0.0.1" | "localhost"))
        && uri.port_u16() == Some(port);
    let path = uri
        .path_and_query()
        .filter(|p| ours && p.path().starts_with("/overlay/"))?;
    Some(path.as_str().to_owned())
}

#[derive(Serialize, ToSchema)]
pub(crate) struct ObsSources {
    /// Whether OBS is connected; the rest is empty while it is not.
    connected: bool,
    /// Whether OBS runs on this computer; overlays can be added to scenes only then, since the Host's addresses
    /// open nowhere else.
    local: bool,
    /// The scene on air.
    #[schema(required)]
    live_scene: Option<String>,
    /// Every scene, in OBS's order.
    scenes: Vec<String>,
    /// The browser sources that show one of this Host's overlays; other browser sources are left out.
    sources: Vec<OverlaySource>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct OverlaySource {
    /// The source's name in OBS.
    name: String,
    /// The overlay it shows: its path and query, e.g. `/overlay/leaderboard/?board=bits&period=stream`.
    overlay: String,
    /// The scenes it is an item of (items inside a group are not counted).
    scenes: Vec<String>,
}

/// Where this Host's overlays are in OBS: one entry per browser source showing one, with the scenes it is in.
#[utoipa::path(
    get,
    path = "/api/admin/obs/sources",
    tag = "admin",
    responses(
        (status = 200, body = ObsSources),
        (status = 502, body = ErrorBody),
    )
)]
pub(crate) async fn overlay_sources(State(link): State<Arc<ObsLink>>) -> Response {
    let browser = match link.browser_sources().await {
        Ok(sources) => sources,
        Err(StreamingError::NotConnected) => {
            return Json(ObsSources {
                connected: false,
                local: link.obs_is_local(),
                live_scene: None,
                scenes: Vec::new(),
                sources: Vec::new(),
            })
            .into_response();
        }
        Err(e) => return command_response(Err(e)),
    };
    let state = link.state();
    let sources = browser
        .into_iter()
        .filter_map(|source| {
            let overlay = own_overlay(&source.url, link.port)?;
            let scenes = state
                .scenes
                .iter()
                .filter(|s| s.items.iter().any(|i| i.source == source.name))
                .map(|s| s.name.clone())
                .collect();
            Some(OverlaySource {
                name: source.name,
                overlay,
                scenes,
            })
        })
        .collect();
    Json(ObsSources {
        connected: true,
        local: link.obs_is_local(),
        live_scene: state.scene.clone(),
        scenes: state.scenes.iter().map(|s| s.name.clone()).collect(),
        sources,
    })
    .into_response()
}

/// The browser sources showing `overlay`, by name. As in the admin UI, an overlay given with a query (a
/// leaderboard's board and period) matches exactly; one without matches its path whatever the query.
fn sources_of(browser: Vec<BrowserSource>, overlay: &str, port: u16) -> Vec<String> {
    let exact = overlay.contains('?');
    browser
        .into_iter()
        .filter(|s| {
            own_overlay(&s.url, port).is_some_and(|o| {
                if exact {
                    o == overlay
                } else {
                    o.split('?').next() == Some(overlay)
                }
            })
        })
        .map(|s| s.name)
        .collect()
}

/// `name`, or `name 2`, `name 3`... when a source already has it.
fn free_name(name: &str, taken: &[String]) -> String {
    (1..)
        .map(|n| {
            if n == 1 {
                name.to_owned()
            } else {
                format!("{name} {n}")
            }
        })
        .find(|candidate| !taken.contains(candidate))
        .expect("an unused name")
}

/// An overlay path this Host serves, with its query: `/overlay/...`, no spaces or control characters.
fn valid_overlay(overlay: &str) -> bool {
    overlay.starts_with("/overlay/")
        && overlay.len() <= 200
        && !overlay.chars().any(|c| c.is_whitespace() || c.is_control())
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct AddOverlayBody {
    scene: String,
    /// The overlay's path and query, e.g. `/overlay/chat/`.
    overlay: String,
    /// The name for a new browser source (1-100 characters); `name 2` and so on when it is taken.
    name: String,
    /// The new browser source's width, 1-8192.
    width: u32,
    /// The new browser source's height, 1-8192.
    height: u32,
}

/// Puts an overlay in a scene: the browser source already showing it gets a new item there, or a new browser
/// source opens the overlay's address on this Host (never with a token). Only while OBS runs on this computer.
#[utoipa::path(
    post,
    path = "/api/admin/obs/sources",
    tag = "admin",
    request_body = AddOverlayBody,
    responses(
        (status = 204),
        (status = 400, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(crate) async fn add_overlay(
    State(link): State<Arc<ObsLink>>,
    Json(body): Json<AddOverlayBody>,
) -> Response {
    let sized = |n| (1..=8192).contains(&n);
    let name = body.name.trim();
    if !valid_overlay(&body.overlay)
        || !(1..=100).contains(&name.chars().count())
        || !sized(body.width)
        || !sized(body.height)
    {
        return error(StatusCode::BAD_REQUEST, "obs.invalid_source");
    }
    if !link.obs_is_local() {
        return error(StatusCode::CONFLICT, "obs.not_local");
    }
    let browser = match link.browser_sources().await {
        Ok(browser) => browser,
        Err(e) => return command_response(Err(e)),
    };
    let state = link.state();
    let mut taken: Vec<String> = browser.iter().map(|s| s.name.clone()).collect();
    taken.extend(state.inputs.iter().map(|i| i.name.clone()));
    taken.extend(
        state
            .scenes
            .iter()
            .flat_map(|s| s.items.iter().map(|i| i.source.clone())),
    );
    let existing = sources_of(browser, &body.overlay, link.port);
    let in_scene = |source: &String| {
        state
            .scenes
            .iter()
            .any(|s| s.name == body.scene && s.items.iter().any(|i| &i.source == source))
    };
    let result = if existing.iter().any(in_scene) {
        Ok(())
    } else if let Some(source) = existing.first() {
        link.add_source(&body.scene, source).await
    } else {
        let url = format!("http://127.0.0.1:{}{}", link.port, body.overlay);
        let name = free_name(name, &taken);
        link.create_browser_source(&body.scene, &name, &url, (body.width, body.height))
            .await
    };
    command_response(result)
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct RemoveOverlayQuery {
    scene: String,
    /// The overlay's path and query, as in `GET /api/admin/obs/sources`.
    overlay: String,
}

/// Takes an overlay out of a scene: removes that scene's items whose source shows the overlay. Items of other
/// sources are never touched; the sources stay in the other scenes.
#[utoipa::path(
    delete,
    path = "/api/admin/obs/sources",
    tag = "admin",
    params(RemoveOverlayQuery),
    responses(
        (status = 204),
        (status = 400, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(crate) async fn remove_overlay(
    State(link): State<Arc<ObsLink>>,
    Query(query): Query<RemoveOverlayQuery>,
) -> Response {
    if !valid_overlay(&query.overlay) {
        return error(StatusCode::BAD_REQUEST, "obs.invalid_source");
    }
    let browser = match link.browser_sources().await {
        Ok(browser) => browser,
        Err(e) => return command_response(Err(e)),
    };
    let sources = sources_of(browser, &query.overlay, link.port);
    let items: Vec<i64> = link
        .state()
        .scenes
        .iter()
        .filter(|s| s.name == query.scene)
        .flat_map(|s| s.items.iter())
        .filter(|i| sources.contains(&i.source))
        .map(|i| i.id)
        .collect();
    if items.is_empty() {
        return error(StatusCode::NOT_FOUND, "obs.not_found");
    }
    for item in items {
        if let Err(e) = link.remove_item(&query.scene, item).await {
            return command_response(Err(e));
        }
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
        let _link = ObsLink::start(dir.clone(), 0, Arc::new(NoPassword), bus.clone());

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

    #[tokio::test]
    async fn a_burst_of_state_changes_goes_out_as_few_events_ending_with_the_latest() {
        let dir = std::env::temp_dir().join(format!("st-obs-burst-{}", std::process::id()));
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        settings::update(&dir, |s| s.obs.port = port).unwrap();
        let bus = Arc::new(EventBus::default());
        let link = ObsLink::start(dir.clone(), 0, Arc::new(NoPassword), bus.clone());
        tokio::time::sleep(Duration::from_millis(200)).await;
        let (_, mut events) = bus.subscribe();
        for n in 0..50 {
            link.state.send_replace(StreamingState {
                scene: Some(format!("S{n}")),
                ..StreamingState::default()
            });
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
        let mut states = Vec::new();
        while let Ok(e) = events.try_recv() {
            let e: serde_json::Value = serde_json::from_str(&e).unwrap();
            if e["type"] == STATE_EVENT {
                states.push(
                    e["payload"]["scene"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                );
            }
        }
        assert!(states.len() <= 4, "{states:?}");
        assert_eq!(states.last().map(String::as_str), Some("S49"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn an_unused_obs_gets_no_connection_until_used_again() {
        let dir = std::env::temp_dir().join(format!("st-obs-unused-{}", std::process::id()));
        settings::update(&dir, |s| {
            s.services = Some(toolkit_core::settings::ServiceUse {
                unused: vec![ExternalService::Obs],
                parked: vec![],
            })
        })
        .unwrap();
        let bus = Arc::new(EventBus::default());
        let link = ObsLink::start(dir.clone(), 0, Arc::new(NoPassword), bus);
        assert!(
            link.task.lock().unwrap().is_none(),
            "no connection while unused"
        );
        assert_eq!(
            *link.status.borrow(),
            ObsStatus::UNUSED,
            "not a stale connected light"
        );
        settings::update(&dir, |s| s.services = None).unwrap();
        link.restart();
        assert!(
            link.task.lock().unwrap().is_some(),
            "connects once used again"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn only_own_overlay_addresses_are_kept() {
        let own = |url| own_overlay(url, 47810);
        assert_eq!(
            own("http://127.0.0.1:47810/overlay/chat/").as_deref(),
            Some("/overlay/chat/")
        );
        assert_eq!(
            own("http://localhost:47810/overlay/leaderboard/?board=bits&period=stream").as_deref(),
            Some("/overlay/leaderboard/?board=bits&period=stream")
        );
        for other in [
            "http://127.0.0.1:47811/overlay/chat/",
            "https://127.0.0.1:47810/overlay/chat/",
            "http://example.com:47810/overlay/chat/",
            "http://127.0.0.1:47810/api/admin/obs",
            "https://streamlabs.com/alert-box/v3/secret-token",
            "",
            "not a url",
        ] {
            assert_eq!(own(other), None, "{other}");
        }
    }

    #[test]
    fn a_taken_name_gets_a_number() {
        let taken = ["Chat".to_owned(), "Chat 2".to_owned()];
        assert_eq!(free_name("Chat", &taken), "Chat 3");
        assert_eq!(free_name("Cards", &taken), "Cards");
    }

    #[test]
    fn sources_match_an_overlay_as_the_admin_ui_does() {
        let browser = || {
            [
                ("Chat", "http://127.0.0.1:7878/overlay/chat/?x=1"),
                (
                    "Bits",
                    "http://127.0.0.1:7878/overlay/leaderboard/?board=bits&period=stream",
                ),
                ("Chat elsewhere", "http://example.com:7878/overlay/chat/"),
            ]
            .map(|(name, url)| BrowserSource {
                name: name.into(),
                url: url.into(),
            })
            .into()
        };
        assert_eq!(sources_of(browser(), "/overlay/chat/", 7878), ["Chat"]);
        assert_eq!(
            sources_of(
                browser(),
                "/overlay/leaderboard/?board=bits&period=stream",
                7878
            ),
            ["Bits"]
        );
        assert!(
            sources_of(
                browser(),
                "/overlay/leaderboard/?board=chat&period=stream",
                7878
            )
            .is_empty()
        );
    }

    #[test]
    fn overlay_paths_are_checked() {
        assert!(valid_overlay(
            "/overlay/leaderboard/?board=bits&period=stream"
        ));
        for bad in [
            "/api/admin/obs",
            "/overlay/chat/ x",
            "/overlay/\nchat",
            "overlay/chat/",
        ] {
            assert!(!valid_overlay(bad), "{bad}");
        }
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
