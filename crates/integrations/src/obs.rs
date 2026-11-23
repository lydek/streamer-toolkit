//! OBS WebSocket (obs-websocket 5) through `obws`: the connection status, reconnecting with backoff, and the
//! streaming-software state (scenes, items, inputs, outputs), read in full on connect and then kept current from
//! OBS's events, and the commands features send through the same connection. OBS not running is a normal state: the
//! status says so and only changes are logged.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use futures_util::StreamExt;
use obws::common::{Alignment, BoundsType};
use obws::{
    Client,
    client::{ConnectConfig, HandshakeError},
    error::Error as ObwsError,
    events::Event,
    requests::{
        EventSubscription,
        inputs::Create,
        inputs::InputId,
        inputs::SetSettings,
        scene_items::{
            Bounds as ObsBounds, CreateSceneItem, Crop as ObsCrop, Position, Scale,
            SceneItemTransform, SetEnabled, SetIndex, SetLocked, SetTransform,
        },
        scenes::SceneId,
        sources::SourceId,
    },
    responses::{StatusCode, scene_items::SourceType},
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use toolkit_core::streaming::{
    Align, Bounds, BrowserSource, Canvas, Input, ItemCrop, ItemSize, ItemTransform, Placement,
    Scene, SceneItem, StreamingError, StreamingState, StreamingStats, TransformChange,
};

use crate::secrets::SecretStore;

/// Covers the whole connect: TCP, upgrade, handshake and version check (obws times out only the first two).
// shortcut: 1 s in tests keeps the stalled-handshake test fast; make it a field if callers ever need to tune it.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(if cfg!(test) { 1 } else { 10 });
/// obs-websocket close code for a missing or wrong password.
const AUTHENTICATION_FAILED: u16 = 4009;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObsState {
    Connected,
    /// Not connected yet, or retrying on its own (e.g. OBS is closed).
    Reconnecting,
    /// Retrying, but the broadcaster has to fix something first (e.g. the password).
    Error,
}

/// What the OBS status light shows; `reason` is a stable code, never a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ObsStatus {
    pub state: ObsState,
    pub reason: Option<&'static str>,
}

impl ObsStatus {
    pub const CONNECTING: Self = Self::reconnecting(None);
    /// The broadcaster does not use OBS: no connection, and none tried.
    pub const UNUSED: Self = Self::reconnecting(Some("obs.unused"));
    const CONNECTED: Self = Self {
        state: ObsState::Connected,
        reason: None,
    };

    const fn reconnecting(reason: Option<&'static str>) -> Self {
        Self {
            state: ObsState::Reconnecting,
            reason,
        }
    }

    const fn error(reason: &'static str) -> Self {
        Self {
            state: ObsState::Error,
            reason: Some(reason),
        }
    }
}

/// The connected client, shared so commands go through the connection the state follows; empty while not connected.
#[derive(Default)]
pub struct ObsControl(Mutex<Option<Arc<Client>>>);

impl ObsControl {
    /// Forgets the client, e.g. when new settings replace the connection.
    pub fn clear(&self) {
        *self.0.lock().unwrap() = None;
    }

    fn client(&self) -> Result<Arc<Client>, StreamingError> {
        self.0
            .lock()
            .unwrap()
            .clone()
            .ok_or(StreamingError::NotConnected)
    }

    pub async fn switch_scene(&self, scene: &str) -> Result<(), StreamingError> {
        let client = self.client()?;
        client
            .scenes()
            .set_current_program_scene(SceneId::Name(scene))
            .await
            .map_err(command_error)
    }

    pub async fn set_item_enabled(
        &self,
        scene: &str,
        item: i64,
        enabled: bool,
    ) -> Result<(), StreamingError> {
        let client = self.client()?;
        client
            .scene_items()
            .set_enabled(SetEnabled {
                scene: SceneId::Name(scene),
                item_id: item,
                enabled,
            })
            .await
            .map_err(command_error)
    }

    /// CPU and render counters (`GetStats`), and the stream output's (`GetStreamStatus`).
    pub async fn stats(&self) -> Result<StreamingStats, StreamingError> {
        let client = self.client()?;
        let general = client.general().stats().await.map_err(command_error)?;
        let stream = client.streaming().status().await.map_err(command_error)?;
        Ok(StreamingStats {
            cpu_percent: general.cpu_usage,
            render_skipped: general.render_skipped_frames,
            render_total: general.render_total_frames,
            streaming: stream.active,
            stream_skipped: stream.skipped_frames,
            stream_total: stream.total_frames,
            stream_bytes: stream.bytes,
        })
    }

    pub async fn set_item_locked(
        &self,
        scene: &str,
        item: i64,
        locked: bool,
    ) -> Result<(), StreamingError> {
        let client = self.client()?;
        client
            .scene_items()
            .set_locked(SetLocked {
                scene: SceneId::Name(scene),
                item_id: item,
                locked,
            })
            .await
            .map_err(command_error)
    }

    /// Every browser source (`GetInputList` by kind) and its `url` setting (`GetInputSettings`).
    pub async fn browser_sources(&self) -> Result<Vec<BrowserSource>, StreamingError> {
        let client = self.client()?;
        let inputs = client
            .inputs()
            .list(Some(BROWSER_SOURCE))
            .await
            .map_err(command_error)?;
        let mut sources = Vec::with_capacity(inputs.len());
        for input in inputs {
            let settings = client
                .inputs()
                .settings::<serde_json::Value>(InputId::Name(&input.id.name))
                .await
                .map_err(command_error)?;
            let url = settings.settings["url"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            sources.push(BrowserSource {
                name: input.id.name,
                url,
            });
        }
        Ok(sources)
    }
}

impl ObsControl {
    /// `CreateInput` of kind `browser_source` with its `url`, `width` and `height`, as an item of `scene`.
    pub async fn create_browser_source(
        &self,
        scene: &str,
        name: &str,
        url: &str,
        (width, height): (u32, u32),
    ) -> Result<(), StreamingError> {
        let client = self.client()?;
        client
            .inputs()
            .create(Create {
                scene: SceneId::Name(scene),
                input: name,
                kind: BROWSER_SOURCE,
                settings: Some(serde_json::json!({"url": url, "width": width, "height": height})),
                enabled: Some(true),
            })
            .await
            .map(drop)
            .map_err(command_error)
    }

    /// `CreateSceneItem`: the existing source `source` as a new item of `scene`.
    pub async fn add_source(&self, scene: &str, source: &str) -> Result<(), StreamingError> {
        let client = self.client()?;
        client
            .scene_items()
            .create(CreateSceneItem {
                scene: SceneId::Name(scene),
                source: SourceId::Name(source),
                enabled: Some(true),
            })
            .await
            .map(drop)
            .map_err(command_error)
    }

    /// `SetSceneItemIndex`. OBS counts from the bottom item, so the front-to-back position is turned around
    /// against the scene's item count.
    pub async fn set_item_position(
        &self,
        scene: &str,
        item: i64,
        position: u32,
    ) -> Result<(), StreamingError> {
        let client = self.client()?;
        let count = client
            .scene_items()
            .list(SceneId::Name(scene))
            .await
            .map_err(command_error)?
            .len();
        let index = u32::try_from(count)
            .ok()
            .and_then(|count| count.checked_sub(position + 1))
            .ok_or(StreamingError::NotFound)?;
        client
            .scene_items()
            .set_index(SetIndex {
                scene: SceneId::Name(scene),
                item_id: item,
                index,
            })
            .await
            .map_err(command_error)
    }

    /// `GetVideoSettings` for the canvas, then `GetSceneItemList` and each item's `GetSceneItemTransform`.
    pub async fn placements(
        &self,
        scene: &str,
    ) -> Result<(Canvas, Vec<(i64, Placement, ItemSize)>), StreamingError> {
        let client = self.client()?;
        let video = client
            .config()
            .video_settings()
            .await
            .map_err(command_error)?;
        let items = client
            .scene_items()
            .list(SceneId::Name(scene))
            .await
            .map_err(command_error)?;
        let mut placements = Vec::with_capacity(items.len());
        for item in items {
            let t = client
                .scene_items()
                .transform(SceneId::Name(scene), item.id)
                .await
                .map_err(command_error)?;
            placements.push((
                item.id,
                Placement {
                    x: t.position_x,
                    y: t.position_y,
                    align: align_of(t.alignment),
                },
                shown_size(&t),
            ));
        }
        let canvas = Canvas {
            width: video.base_width,
            height: video.base_height,
        };
        Ok((canvas, placements))
    }

    /// `SetSceneItemTransform` with the position and alignment only; scale, rotation and crop stay.
    pub async fn set_placement(
        &self,
        scene: &str,
        item: i64,
        placement: Placement,
    ) -> Result<(), StreamingError> {
        let client = self.client()?;
        client
            .scene_items()
            .set_transform(SetTransform {
                scene: SceneId::Name(scene),
                item_id: item,
                transform: SceneItemTransform {
                    position: Some(Position {
                        x: Some(placement.x),
                        y: Some(placement.y),
                    }),
                    alignment: Some(alignment_of(placement.align)),
                    ..Default::default()
                },
            })
            .await
            .map_err(command_error)
    }

    /// `GetSceneItemTransform`, then `SetSceneItemTransform` with what sets the size on the canvas: the bounding box
    /// when one is on, else the scale of the source's size less the crop (flips kept); position, rotation and crop
    /// stay.
    pub async fn set_item_size(
        &self,
        scene: &str,
        item: i64,
        size: ItemSize,
    ) -> Result<(), StreamingError> {
        let client = self.client()?;
        let t = client
            .scene_items()
            .transform(SceneId::Name(scene), item)
            .await
            .map_err(command_error)?;
        let transform = if t.bounds_type == BoundsType::None {
            let shown_w = t.source_width - (t.crop_left + t.crop_right) as f32;
            let shown_h = t.source_height - (t.crop_top + t.crop_bottom) as f32;
            if shown_w <= 0.0 || shown_h <= 0.0 {
                return Err(StreamingError::NotFound);
            }
            // A flip is a negative scale: the new scale keeps each side's sign.
            SceneItemTransform {
                scale: Some(Scale {
                    x: Some(size.width / shown_w * t.scale_x.signum()),
                    y: Some(size.height / shown_h * t.scale_y.signum()),
                }),
                ..Default::default()
            }
        } else {
            SceneItemTransform {
                bounds: Some(ObsBounds {
                    width: Some(size.width),
                    height: Some(size.height),
                    ..Default::default()
                }),
                ..Default::default()
            }
        };
        client
            .scene_items()
            .set_transform(SetTransform {
                scene: SceneId::Name(scene),
                item_id: item,
                transform,
            })
            .await
            .map_err(command_error)
    }

    /// `GetSceneItemTransform`: rotation, flips (negative scales), crop and bounding box type.
    pub async fn item_transform(
        &self,
        scene: &str,
        item: i64,
    ) -> Result<ItemTransform, StreamingError> {
        let client = self.client()?;
        let t = client
            .scene_items()
            .transform(SceneId::Name(scene), item)
            .await
            .map_err(command_error)?;
        Ok(ItemTransform {
            rotation: t.rotation,
            flip_h: t.scale_x < 0.0,
            flip_v: t.scale_y < 0.0,
            crop: ItemCrop {
                top: t.crop_top,
                right: t.crop_right,
                bottom: t.crop_bottom,
                left: t.crop_left,
            },
            bounds: bounds_of(t.bounds_type),
        })
    }

    /// `SetSceneItemTransform` with only the parts `change` gives. A flip turns a scale's sign; a bounding box
    /// turned on starts at the item's size now, so nothing jumps.
    pub async fn set_item_transform(
        &self,
        scene: &str,
        item: i64,
        change: TransformChange,
    ) -> Result<(), StreamingError> {
        let client = self.client()?;
        let t = client
            .scene_items()
            .transform(SceneId::Name(scene), item)
            .await
            .map_err(command_error)?;
        let flip = |want: Option<bool>, scale: f32| {
            want.map(|f| if f { -scale.abs() } else { scale.abs() })
        };
        let scale = match (
            flip(change.flip_h, t.scale_x),
            flip(change.flip_v, t.scale_y),
        ) {
            (None, None) => None,
            (x, y) => Some(Scale { x, y }),
        };
        let bounds = change.bounds.map(|b| ObsBounds {
            r#type: Some(bounds_type_of(b)),
            width: (b != Bounds::None).then_some(shown_size(&t).width.max(1.0)),
            height: (b != Bounds::None).then_some(shown_size(&t).height.max(1.0)),
            ..Default::default()
        });
        let crop = change.crop.map(|c| ObsCrop {
            top: Some(c.top),
            right: Some(c.right),
            bottom: Some(c.bottom),
            left: Some(c.left),
        });
        client
            .scene_items()
            .set_transform(SetTransform {
                scene: SceneId::Name(scene),
                item_id: item,
                transform: SceneItemTransform {
                    rotation: change.rotation,
                    scale,
                    bounds,
                    crop,
                    ..Default::default()
                },
            })
            .await
            .map_err(command_error)
    }

    /// `GetInputSettings` over its kind's `GetInputDefaultSettings`: the input's kind and settings object, fields
    /// never changed included (OBS leaves them out of the settings).
    pub async fn input_settings(
        &self,
        input: &str,
    ) -> Result<(String, serde_json::Map<String, serde_json::Value>), StreamingError> {
        let client = self.client()?;
        let got = client
            .inputs()
            .settings::<serde_json::Map<String, serde_json::Value>>(InputId::Name(input))
            .await
            .map_err(command_error)?;
        let mut settings = client
            .inputs()
            .default_settings::<serde_json::Map<String, serde_json::Value>>(&got.kind)
            .await
            .map_err(command_error)?;
        settings.extend(got.settings);
        Ok((got.kind, settings))
    }

    /// `SetInputSettings` with `overlay`, so fields not given keep their values.
    pub async fn set_input_settings(
        &self,
        input: &str,
        settings: serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), StreamingError> {
        let client = self.client()?;
        client
            .inputs()
            .set_settings(SetSettings {
                input: InputId::Name(input),
                settings: &settings,
                overlay: Some(true),
            })
            .await
            .map_err(command_error)
    }

    /// `PressInputPropertiesButton`.
    pub async fn press_input_button(
        &self,
        input: &str,
        property: &str,
    ) -> Result<(), StreamingError> {
        let client = self.client()?;
        client
            .inputs()
            .press_properties_button(InputId::Name(input), property)
            .await
            .map_err(command_error)
    }

    /// `GetInputPropertiesListPropertyItems`, the enabled items only.
    pub async fn input_list_items(
        &self,
        input: &str,
        property: &str,
    ) -> Result<Vec<(String, serde_json::Value)>, StreamingError> {
        let client = self.client()?;
        let items = client
            .inputs()
            .properties_list_property_items(InputId::Name(input), property)
            .await
            .map_err(command_error)?;
        Ok(items
            .into_iter()
            .filter(|i| i.enabled)
            .map(|i| (i.name, i.value))
            .collect())
    }

    /// `SetStudioModeEnabled`.
    pub async fn set_studio_mode(&self, enabled: bool) -> Result<(), StreamingError> {
        let client = self.client()?;
        client
            .ui()
            .set_studio_mode_enabled(enabled)
            .await
            .map_err(command_error)
    }

    /// `SetCurrentPreviewScene`.
    pub async fn set_preview(&self, scene: &str) -> Result<(), StreamingError> {
        let client = self.client()?;
        client
            .scenes()
            .set_current_preview_scene(SceneId::Name(scene))
            .await
            .map_err(command_error)
    }

    /// `SetCurrentSceneTransition` when one is given, then `TriggerStudioModeTransition`.
    pub async fn transition(&self, transition: Option<&str>) -> Result<(), StreamingError> {
        let client = self.client()?;
        if let Some(name) = transition {
            client
                .transitions()
                .set_current(name)
                .await
                .map_err(command_error)?;
        }
        client.transitions().trigger().await.map_err(command_error)
    }

    /// `RemoveSceneItem`.
    pub async fn remove_item(&self, scene: &str, item: i64) -> Result<(), StreamingError> {
        let client = self.client()?;
        client
            .scene_items()
            .remove(SceneId::Name(scene), item)
            .await
            .map_err(command_error)
    }
}

/// The item's size on the canvas: its bounding box when one is on, else the source's size less the crop, scaled
/// (OBS's own `width`/`height` leave the crop in and carry a flip's sign).
fn shown_size(t: &obws::responses::scene_items::SceneItemTransform) -> ItemSize {
    if t.bounds_type != BoundsType::None {
        return ItemSize {
            width: t.bounds_width,
            height: t.bounds_height,
        };
    }
    ItemSize {
        width: ((t.source_width - (t.crop_left + t.crop_right) as f32) * t.scale_x.abs()).max(0.0),
        height: ((t.source_height - (t.crop_top + t.crop_bottom) as f32) * t.scale_y.abs())
            .max(0.0),
    }
}

fn bounds_of(b: BoundsType) -> Bounds {
    match b {
        BoundsType::None => Bounds::None,
        BoundsType::Stretch => Bounds::Stretch,
        BoundsType::ScaleInner => Bounds::ScaleInner,
        BoundsType::ScaleOuter => Bounds::ScaleOuter,
        BoundsType::ScaleToWidth => Bounds::ScaleToWidth,
        BoundsType::ScaleToHeight => Bounds::ScaleToHeight,
        BoundsType::MaxOnly => Bounds::MaxOnly,
        _ => Bounds::None,
    }
}

fn bounds_type_of(b: Bounds) -> BoundsType {
    match b {
        Bounds::None => BoundsType::None,
        Bounds::Stretch => BoundsType::Stretch,
        Bounds::ScaleInner => BoundsType::ScaleInner,
        Bounds::ScaleOuter => BoundsType::ScaleOuter,
        Bounds::ScaleToWidth => BoundsType::ScaleToWidth,
        Bounds::ScaleToHeight => BoundsType::ScaleToHeight,
        Bounds::MaxOnly => BoundsType::MaxOnly,
    }
}

/// OBS's alignment flags as the port's nine points; a combination OBS does not use reads as the nearest one.
fn align_of(a: Alignment) -> Align {
    let (left, right) = (a.contains(Alignment::LEFT), a.contains(Alignment::RIGHT));
    let (top, bottom) = (a.contains(Alignment::TOP), a.contains(Alignment::BOTTOM));
    match (top, bottom, left, right) {
        (true, false, true, false) => Align::TopLeft,
        (true, false, false, true) => Align::TopRight,
        (true, false, ..) => Align::Top,
        (false, true, true, false) => Align::BottomLeft,
        (false, true, false, true) => Align::BottomRight,
        (false, true, ..) => Align::Bottom,
        (_, _, true, false) => Align::Left,
        (_, _, false, true) => Align::Right,
        _ => Align::Center,
    }
}

fn alignment_of(a: Align) -> Alignment {
    match a {
        Align::TopLeft => Alignment::TOP | Alignment::LEFT,
        Align::Top => Alignment::TOP,
        Align::TopRight => Alignment::TOP | Alignment::RIGHT,
        Align::Left => Alignment::LEFT,
        Align::Center => Alignment::CENTER,
        Align::Right => Alignment::RIGHT,
        Align::BottomLeft => Alignment::BOTTOM | Alignment::LEFT,
        Align::Bottom => Alignment::BOTTOM,
        Align::BottomRight => Alignment::BOTTOM | Alignment::RIGHT,
    }
}

/// OBS's input kind for a browser source.
const BROWSER_SOURCE: &str = "browser_source";

fn command_error(error: ObwsError) -> StreamingError {
    match error {
        ObwsError::Api {
            code: StatusCode::ResourceNotFound,
            ..
        } => StreamingError::NotFound,
        error => {
            tracing::warn!(%error, "OBS command failed");
            StreamingError::Failed
        }
    }
}

pub struct Obs {
    pub host: String,
    pub port: u16,
    /// The obs-websocket password; empty slot means OBS has authentication turned off.
    pub secrets: Arc<dyn SecretStore>,
    pub status: Arc<watch::Sender<ObsStatus>>,
    /// Empty (unknown scene) while not connected.
    pub state: Arc<watch::Sender<StreamingState>>,
    /// This connection's number; it writes status and state only while `current` still holds it, so a connection
    /// replaced by new settings never writes over the new one.
    pub generation: u64,
    pub current: Arc<AtomicU64>,
    /// Holds this connection's client while it is connected and still the current one.
    pub control: Arc<ObsControl>,
}

impl Obs {
    /// Runs until the task is dropped; host, port or password changes start a new task.
    pub async fn run(self) {
        self.set(ObsStatus::CONNECTING);
        let mut failures = 0u32;
        loop {
            let ended = match self.connect().await {
                Ok(client) => {
                    failures = 0;
                    let client = Arc::new(client);
                    self.share(&client);
                    self.set(ObsStatus::CONNECTED);
                    self.follow(&client).await;
                    self.unshare(&client);
                    self.set_state(&StreamingState::default());
                    ObsStatus::reconnecting(Some("obs.disconnected"))
                }
                Err(status) => status,
            };
            failures += 1;
            self.set(ended);
            tokio::time::sleep(crate::backoff(failures)).await;
        }
    }

    async fn connect(&self) -> Result<Client, ObsStatus> {
        let password = self
            .secrets
            .read()
            .map_err(|_| ObsStatus::error("obs.secret_store_failed"))?
            .map(SecretString::from);
        let config = ConnectConfig {
            host: self.host.as_str(),
            port: self.port,
            dangerous: None,
            password: password.as_ref().map(|p| p.expose_secret()),
            // obws reports the connection closing without a subscription; these keep the state current.
            event_subscriptions: Some(
                EventSubscription::SCENES
                    | EventSubscription::SCENE_ITEMS
                    | EventSubscription::INPUTS
                    | EventSubscription::OUTPUTS
                    | EventSubscription::TRANSITIONS
                    | EventSubscription::UI,
            ),
            broadcast_capacity: 64,
            // Half the budget, so a silent host ends as `Timeout` (unreachable) before the outer limit fires.
            connect_timeout: CONNECT_TIMEOUT / 2,
        };
        match tokio::time::timeout(CONNECT_TIMEOUT, Client::connect_with_config(config)).await {
            Ok(connected) => connected.map_err(|e| classify(&e, password.is_some())),
            // Something answered the upgrade but never finished the obs-websocket handshake.
            Err(_) => Err(ObsStatus::reconnecting(Some("obs.handshake_failed"))),
        }
    }

    /// Lets commands use `client`, unless new settings replaced this connection.
    fn share(&self, client: &Arc<Client>) {
        let mut slot = self.control.0.lock().unwrap();
        if self.live() {
            *slot = Some(client.clone());
        }
    }

    /// Takes `client` back from commands; a newer connection's client stays.
    fn unshare(&self, client: &Arc<Client>) {
        let mut slot = self.control.0.lock().unwrap();
        if slot.as_ref().is_some_and(|c| Arc::ptr_eq(c, client)) {
            *slot = None;
        }
    }

    /// Whether this connection is still the one the settings asked for.
    fn live(&self) -> bool {
        self.current.load(Ordering::SeqCst) == self.generation
    }

    /// Publishes and logs only real changes, so a closed OBS never fills the log.
    fn set(&self, status: ObsStatus) {
        // Checked inside the channel's lock, so a replaced connection cannot write after the switch.
        let changed = self.status.send_if_modified(|current| {
            let changed = self.live() && *current != status;
            if changed {
                *current = status;
            }
            changed
        });
        if changed {
            match status.state {
                ObsState::Error => tracing::warn!(reason = status.reason, "OBS connection error"),
                state => tracing::info!(?state, reason = status.reason, "OBS connection"),
            }
        }
    }
}

impl Obs {
    /// Keeps the state current until obs-websocket closes the connection (OBS exits, the server stops or the
    /// socket drops), or a read fails, which ends the connection so the next one starts fresh.
    async fn follow(&self, client: &Client) {
        // Subscribed before reading, so no change between the read and the first event is lost.
        let Ok(mut events) = client.events() else {
            return;
        };
        let Ok(mut state) = read_state(client).await else {
            return;
        };
        self.set_state(&state);
        while let Some(event) = events.next().await {
            match apply(&mut state, &event) {
                Change::Closed => return,
                Change::None => continue,
                Change::Updated => {}
                Change::Reread => match read_state(client).await {
                    Ok(fresh) => state = fresh,
                    Err(_) => return,
                },
            }
            self.set_state(&state);
        }
    }

    /// Publishes `state` when it changed and this connection is still the current one.
    fn set_state(&self, state: &StreamingState) {
        self.state.send_if_modified(|current| {
            let changed = self.live() && current != state;
            if changed {
                current.clone_from(state);
            }
            changed
        });
    }
}

/// Every scene with its items, every input with its mute state, and whether OBS streams and records.
async fn read_state(client: &Client) -> Result<StreamingState, ObwsError> {
    let list = client.scenes().list().await?;
    let mut scenes = Vec::with_capacity(list.scenes.len());
    for scene in list.scenes {
        let id = SceneId::Uuid(scene.id.uuid);
        let mut items = Vec::new();
        // OBS lists index 0 (the bottom item) first; the port lists front to back.
        for item in client.scene_items().list(id).await?.into_iter().rev() {
            let kind = if item.is_group == Some(true) {
                "group".to_owned()
            } else if item.source_type == SourceType::Scene {
                "scene".to_owned()
            } else {
                item.input_kind.unwrap_or_default()
            };
            items.push(SceneItem {
                id: item.id,
                enabled: client.scene_items().enabled(id, item.id).await?,
                locked: client.scene_items().locked(id, item.id).await?,
                source: item.source_name,
                kind,
            });
        }
        scenes.push(Scene {
            name: scene.id.name,
            items,
        });
    }
    let mut inputs = Vec::new();
    for input in client.inputs().list(None).await? {
        let muted = match client.inputs().muted(InputId::Uuid(input.id.uuid)).await {
            Ok(muted) => muted,
            // An input without audio (an image, a display capture) has no mute state: OBS answers with an error,
            // which must not end the connection. Only inputs that can be muted are listed.
            Err(ObwsError::Api {
                code: StatusCode::InvalidResourceState,
                ..
            }) => continue,
            Err(e) => return Err(e),
        };
        inputs.push(Input {
            muted,
            name: input.id.name,
        });
    }
    let studio_mode = client.ui().studio_mode_enabled().await?;
    let transitions = client.transitions().list().await?;
    Ok(StreamingState {
        scene: list.current_program_scene.map(|s| s.name),
        scenes,
        inputs,
        streaming: client.streaming().status().await?.active,
        recording: client.recording().status().await?.active,
        studio_mode,
        preview: list
            .current_preview_scene
            .filter(|_| studio_mode)
            .map(|s| s.name),
        transitions: transitions
            .transitions
            .into_iter()
            .map(|t| t.id.name)
            .collect(),
        transition: transitions.current_scene_transition.map(|t| t.name),
    })
}

#[derive(Debug, PartialEq, Eq)]
enum Change {
    None,
    Updated,
    /// Scenes, items or inputs were added, removed, renamed or reordered: read everything again.
    Reread,
    Closed,
}

fn apply(state: &mut StreamingState, event: &Event) -> Change {
    match event {
        Event::ServerStopping | Event::ServerStopped => Change::Closed,
        Event::CurrentProgramSceneChanged { id } => {
            state.scene = Some(id.name.clone());
            Change::Updated
        }
        // Items inside a group are not listed, so their changes are ignored.
        Event::SceneItemEnableStateChanged {
            scene,
            item_id,
            enabled,
        } => match item_mut(state, &scene.name, *item_id) {
            Some(item) => {
                item.enabled = *enabled;
                Change::Updated
            }
            None => Change::None,
        },
        Event::SceneItemLockStateChanged {
            scene,
            item_id,
            locked,
        } => match item_mut(state, &scene.name, *item_id) {
            Some(item) => {
                item.locked = *locked;
                Change::Updated
            }
            None => Change::None,
        },
        Event::InputMuteStateChanged { id, muted } => {
            match state.inputs.iter_mut().find(|i| i.name == id.name) {
                Some(input) => {
                    input.muted = *muted;
                    Change::Updated
                }
                None => Change::Reread,
            }
        }
        Event::StreamStateChanged { active, .. } => {
            state.streaming = *active;
            Change::Updated
        }
        Event::RecordStateChanged { active, .. } => {
            state.recording = *active;
            Change::Updated
        }
        // Turning studio mode on picks a preview scene: read it with the rest.
        Event::StudioModeStateChanged { enabled: true } => Change::Reread,
        Event::StudioModeStateChanged { enabled: false } => {
            state.studio_mode = false;
            state.preview = None;
            Change::Updated
        }
        Event::CurrentPreviewSceneChanged { id } => {
            state.preview = Some(id.name.clone());
            Change::Updated
        }
        Event::CurrentSceneTransitionChanged { id } => {
            state.transition = Some(id.name.clone());
            Change::Updated
        }
        Event::SceneListChanged { .. }
        | Event::SceneCreated { .. }
        | Event::SceneRemoved { .. }
        | Event::SceneNameChanged { .. }
        | Event::SceneItemCreated { .. }
        | Event::SceneItemRemoved { .. }
        | Event::SceneItemListReindexed { .. }
        | Event::InputCreated { .. }
        | Event::InputRemoved { .. }
        | Event::InputNameChanged { .. } => Change::Reread,
        _ => Change::None,
    }
}

fn item_mut<'a>(state: &'a mut StreamingState, scene: &str, id: u64) -> Option<&'a mut SceneItem> {
    state
        .scenes
        .iter_mut()
        .filter(|s| s.name == scene)
        .flat_map(|s| s.items.iter_mut())
        .find(|i| u64::try_from(i.id).ok() == Some(id))
}

fn classify(error: &ObwsError, has_password: bool) -> ObsStatus {
    match error {
        ObwsError::Handshake(HandshakeError::ConnectionClosed(Some(details)))
            if u16::from(details.code) == AUTHENTICATION_FAILED =>
        {
            ObsStatus::error(if has_password {
                "obs.auth_failed"
            } else {
                "obs.password_required"
            })
        }
        ObwsError::ObsStudioVersion(..)
        | ObwsError::ObsWebsocketVersion(..)
        | ObwsError::RpcVersion { .. } => ObsStatus::error("obs.unsupported_version"),
        ObwsError::InvalidUri(_) => ObsStatus::error("obs.invalid_address"),
        ObwsError::Connect(_) | ObwsError::Timeout => {
            ObsStatus::reconnecting(Some("obs.unreachable"))
        }
        _ => ObsStatus::reconnecting(Some("obs.handshake_failed")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::memory::MemoryStore;
    use axum::{
        Router,
        extract::{State, WebSocketUpgrade, ws},
        response::Response,
        routing::any,
    };
    use serde_json::{Value, json};
    use std::sync::Mutex;
    use tokio::sync::Notify;

    /// A minimal obs-websocket 5 server: handshake, `GetVersion`, the requests the state is read with (one scene
    /// "Live" with a camera under a nested scene, one microphone), the commands (recorded; scene "Missing" answers
    /// not found), events on demand, and closing on demand like OBS exiting.
    #[derive(Clone, Default)]
    struct FakeObs {
        require_auth: bool,
        /// Reads `Identify` and never answers, like a stuck OBS or another WebSocket service.
        stall: bool,
        identify: Arc<Mutex<Vec<Value>>>,
        exit: Arc<Notify>,
        /// Events waiting to be sent: `(eventType, eventData)`.
        pending: Arc<Mutex<Vec<(&'static str, Value)>>>,
        push: Arc<Notify>,
        scene_lists: Arc<Mutex<u32>>,
        /// `Set…` requests as `(requestType, requestData)`.
        commands: Arc<Mutex<Vec<(String, Value)>>>,
    }

    const LIVE: &str = "00000000-0000-0000-0000-000000000001";
    const MIC: &str = "00000000-0000-0000-0000-000000000002";
    /// An image: OBS answers its mute state with an error, as for every input without audio.
    const LOGO: &str = "00000000-0000-0000-0000-000000000003";
    const CHAT: &str = "00000000-0000-0000-0000-000000000004";

    impl FakeObs {
        fn event(&self, kind: &'static str, data: Value) {
            self.pending.lock().unwrap().push((kind, data));
            self.push.notify_one();
        }

        fn answer(&self, request_type: &str, data: &Value) -> Value {
            match request_type {
                "GetInputList" if data["inputKind"] == "browser_source" => json!({"inputs": [{
                    "inputName": "Chat", "inputUuid": CHAT, "inputKind": "browser_source",
                    "unversionedInputKind": "browser_source"}]}),
                "GetInputPropertiesListPropertyItems" => json!({"propertyItems": [
                    {"itemName": "Cam", "itemEnabled": true, "itemValue": "cam1"},
                    {"itemName": "Busy", "itemEnabled": false, "itemValue": "cam2"}]}),
                "GetInputDefaultSettings" => {
                    json!({"defaultInputSettings": {"width": 800, "height": 600}})
                }
                "GetInputSettings" => json!({"inputKind": "browser_source",
                    "inputSettings": {"url": "http://127.0.0.1:47810/overlay/chat/", "width": 800}}),
                "GetSceneList" => {
                    *self.scene_lists.lock().unwrap() += 1;
                    json!({"currentProgramSceneName": "Live", "currentProgramSceneUuid": LIVE,
                           "currentPreviewSceneName": null, "currentPreviewSceneUuid": null,
                           "scenes": [{"sceneName": "Live", "sceneUuid": LIVE, "sceneIndex": 0}]})
                }
                // Index 0 is the bottom item, as OBS lists it.
                "GetSceneItemList" => json!({"sceneItems": [{
                    "sceneItemId": 1, "sceneItemIndex": 0, "sourceName": "Camera",
                    "sourceType": "OBS_SOURCE_TYPE_INPUT", "inputKind": "dshow_input", "isGroup": null}, {
                    "sceneItemId": 2, "sceneItemIndex": 1, "sourceName": "Frame",
                    "sourceType": "OBS_SOURCE_TYPE_SCENE", "inputKind": null, "isGroup": false}]}),
                "GetSceneItemEnabled" => json!({"sceneItemEnabled": true}),
                "GetSceneItemLocked" => json!({"sceneItemLocked": false}),
                t if t.starts_with("Set")
                    || t.starts_with("Remove")
                    || t.starts_with("Trigger") =>
                {
                    Value::Null
                }
                "CreateInput" => json!({"inputUuid": CHAT, "sceneItemId": 9}),
                "CreateSceneItem" => json!({"sceneItemId": 10}),
                "GetStudioModeEnabled" => json!({"studioModeEnabled": false}),
                "GetSceneTransitionList" => json!({"currentSceneTransitionName": "Fade",
                    "currentSceneTransitionUuid": "00000000-0000-0000-0000-000000000010",
                    "currentSceneTransitionKind": "fade_transition", "transitions": [
                    {"transitionName": "Cut", "transitionUuid": "00000000-0000-0000-0000-000000000011",
                     "transitionKind": "cut_transition", "transitionFixed": true, "transitionConfigurable": false},
                    {"transitionName": "Fade", "transitionUuid": "00000000-0000-0000-0000-000000000010",
                     "transitionKind": "fade_transition", "transitionFixed": false, "transitionConfigurable": true}]}),
                "GetVideoSettings" => {
                    json!({"fpsNumerator": 60, "fpsDenominator": 1, "baseWidth": 1920,
                    "baseHeight": 1080, "outputWidth": 1280, "outputHeight": 720})
                }
                // Alignment 5: top left.
                "GetSceneItemTransform" => json!({"sceneItemTransform": {"sourceWidth": 400.0,
                    "sourceHeight": 600.0, "positionX": 40.0, "positionY": 30.0, "rotation": 0.0,
                    "scaleX": 1.0, "scaleY": 1.0, "width": 400.0, "height": 600.0, "alignment": 5,
                    "boundsType": "OBS_BOUNDS_NONE", "boundsAlignment": 0, "boundsWidth": 0.0,
                    "boundsHeight": 0.0, "cropLeft": 0, "cropRight": 0, "cropTop": 0, "cropBottom": 0}}),
                "GetInputList" => json!({"inputs": [{"inputName": "Mic", "inputUuid": MIC,
                    "inputKind": "wasapi_input_capture", "unversionedInputKind": "wasapi_input_capture"}, {
                    "inputName": "Logo", "inputUuid": LOGO,
                    "inputKind": "image_source", "unversionedInputKind": "image_source"}]}),
                "GetInputMute" => json!({"inputMuted": false}),
                "GetStreamStatus" => json!({"outputActive": false, "outputReconnecting": false,
                    "outputTimecode": "00:00:00.000", "outputDuration": 0, "outputCongestion": 0.0,
                    "outputBytes": 0, "outputSkippedFrames": 0, "outputTotalFrames": 0}),
                "GetRecordStatus" => json!({"outputActive": true, "outputPaused": false,
                    "outputTimecode": "00:00:00.000", "outputDuration": 0, "outputBytes": 0}),
                _ => json!({
                    "obsStudioVersion": "31.0.0", "obsWebSocketVersion": "5.5.0", "rpcVersion": 1,
                    "availableRequests": [], "supportedImageFormats": [],
                    "platform": "windows", "platformDescription": "test"}),
            }
        }
    }

    async fn send(socket: &mut ws::WebSocket, value: Value) {
        let _ = socket
            .send(ws::Message::Text(value.to_string().into()))
            .await;
    }

    async fn close(socket: &mut ws::WebSocket, code: u16) {
        let frame = ws::CloseFrame {
            code,
            reason: "".into(),
        };
        let _ = socket.send(ws::Message::Close(Some(frame))).await;
    }

    async fn obs_ws(ws: WebSocketUpgrade, State(fake): State<FakeObs>) -> Response {
        ws.on_upgrade(move |mut socket| async move {
            let auth = fake
                .require_auth
                .then(|| json!({"challenge": "c", "salt": "s"}));
            send(
                &mut socket,
                json!({"op": 0, "d": {"obsWebSocketVersion": "5.5.0", "rpcVersion": 1, "authentication": auth}}),
            )
            .await;
            let Some(Ok(ws::Message::Text(identify))) = socket.recv().await else {
                return;
            };
            fake.identify
                .lock()
                .unwrap()
                .push(serde_json::from_str(identify.as_str()).unwrap());
            if fake.require_auth {
                return close(&mut socket, AUTHENTICATION_FAILED).await;
            }
            if fake.stall {
                while let Some(Ok(_)) = socket.recv().await {}
                return;
            }
            send(&mut socket, json!({"op": 2, "d": {"negotiatedRpcVersion": 1}})).await;
            loop {
                tokio::select! {
                    message = socket.recv() => match message {
                        Some(Ok(ws::Message::Text(text))) => {
                            let request: Value = serde_json::from_str(text.as_str()).unwrap();
                            let request_type = request["d"]["requestType"].as_str().unwrap_or_default();
                            let data = &request["d"]["requestData"];
                            if request_type.starts_with("Set")
                                || request_type.starts_with("Create")
                                || request_type.starts_with("Remove")
                            {
                                fake.commands.lock().unwrap().push((request_type.into(), data.clone()));
                            }
                            let status = if data["sceneName"] == "Missing" {
                                json!({"result": false, "code": 600})
                            } else if request_type == "GetInputMute" && data["inputUuid"] == LOGO {
                                // InvalidResourceState: the input does not support audio.
                                json!({"result": false, "code": 604, "comment": "The specified input does not support audio."})
                            } else {
                                json!({"result": true, "code": 100})
                            };
                            send(&mut socket, json!({"op": 7, "d": {
                                "requestType": request_type,
                                "requestId": request["d"]["requestId"],
                                "requestStatus": status,
                                "responseData": fake.answer(request_type, data)}}))
                            .await;
                        }
                        Some(Ok(_)) => {}
                        _ => return,
                    },
                    _ = fake.push.notified() => {
                        let pending = std::mem::take(&mut *fake.pending.lock().unwrap());
                        for (kind, data) in pending {
                            send(&mut socket, json!({"op": 5, "d": {
                                "eventType": kind, "eventIntent": 1, "eventData": data}})).await;
                        }
                    }
                    _ = fake.exit.notified() => return close(&mut socket, 1001).await,
                }
            }
        })
    }

    async fn start(fake: FakeObs) -> u16 {
        let app = Router::new().route("/", any(obs_ws)).with_state(fake);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        port
    }

    fn run(port: u16, password: Option<&str>) -> watch::Receiver<ObsStatus> {
        run_with_state(port, password).0
    }

    fn run_with_state(
        port: u16,
        password: Option<&str>,
    ) -> (
        watch::Receiver<ObsStatus>,
        watch::Receiver<StreamingState>,
        Arc<ObsControl>,
    ) {
        let secrets = Arc::new(MemoryStore::default());
        *secrets.value.lock().unwrap() = password.map(str::to_owned);
        let (tx, rx) = watch::channel(ObsStatus::CONNECTING);
        let (state_tx, state_rx) = watch::channel(StreamingState::default());
        let control = Arc::new(ObsControl::default());
        tokio::spawn(
            Obs {
                host: "127.0.0.1".into(),
                port,
                secrets,
                status: Arc::new(tx),
                state: Arc::new(state_tx),
                generation: 1,
                current: Arc::new(AtomicU64::new(1)),
                control: control.clone(),
            }
            .run(),
        );
        (rx, state_rx, control)
    }

    #[tokio::test]
    async fn a_replaced_connection_writes_nothing() {
        let fake = FakeObs::default();
        let port = start(fake.clone()).await;
        let (tx, mut rx) = watch::channel(ObsStatus::CONNECTING);
        let (state_tx, state_rx) = watch::channel(StreamingState::default());
        let control = Arc::new(ObsControl::default());
        // Generation 1 when the settings already moved on to 2.
        tokio::spawn(
            Obs {
                host: "127.0.0.1".into(),
                port,
                secrets: Arc::new(MemoryStore::default()),
                status: Arc::new(tx),
                state: Arc::new(state_tx),
                generation: 1,
                current: Arc::new(AtomicU64::new(2)),
                control: control.clone(),
            }
            .run(),
        );
        // It connects (the fake sees it identify) but publishes neither the status nor the state.
        let identified = async {
            while fake.identify.lock().unwrap().is_empty() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        };
        tokio::time::timeout(Duration::from_secs(5), identified)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(*rx.borrow_and_update(), ObsStatus::CONNECTING);
        assert_eq!(*state_rx.borrow(), StreamingState::default());
        assert_eq!(
            control.switch_scene("Live").await,
            Err(StreamingError::NotConnected),
            "nor does it take commands"
        );
    }

    async fn state_reaches(
        rx: &mut watch::Receiver<StreamingState>,
        what: &str,
        done: impl Fn(&StreamingState) -> bool,
    ) {
        let reached = tokio::time::timeout(Duration::from_secs(5), rx.wait_for(|s| done(s)))
            .await
            .is_ok();
        assert!(reached, "{what}: state stayed {:?}", *rx.borrow());
    }

    #[tokio::test]
    async fn state_is_read_on_connect_follows_events_and_resets_on_disconnect() {
        let fake = FakeObs::default();
        let (mut status, mut state, _) = run_with_state(start(fake.clone()).await, None);
        reaches(&mut status, ObsStatus::CONNECTED).await;
        state_reaches(&mut state, "read on connect", |s| s.scene.is_some()).await;
        assert_eq!(
            *state.borrow(),
            StreamingState {
                scene: Some("Live".into()),
                scenes: vec![Scene {
                    name: "Live".into(),
                    // Front to back: the nested scene sits above the camera.
                    items: vec![
                        SceneItem {
                            id: 2,
                            source: "Frame".into(),
                            enabled: true,
                            locked: false,
                            kind: "scene".into(),
                        },
                        SceneItem {
                            id: 1,
                            source: "Camera".into(),
                            enabled: true,
                            locked: false,
                            kind: "dshow_input".into(),
                        },
                    ],
                }],
                // The image has no audio, so no mute state; reading it never ends the connection.
                inputs: vec![Input {
                    name: "Mic".into(),
                    muted: false,
                }],
                streaming: false,
                recording: true,
                studio_mode: false,
                preview: None,
                transitions: vec!["Cut".into(), "Fade".into()],
                transition: Some("Fade".into()),
            }
        );
        assert_eq!(*status.borrow(), ObsStatus::CONNECTED);
        fake.event(
            "CurrentSceneTransitionChanged",
            json!({"transitionName": "Cut", "transitionUuid": "00000000-0000-0000-0000-000000000011"}),
        );
        state_reaches(&mut state, "transition picked", |s| {
            s.transition.as_deref() == Some("Cut")
        })
        .await;

        let scene = |name: &str| json!({"sceneName": name, "sceneUuid": LIVE});
        fake.event("CurrentProgramSceneChanged", scene("BRB"));
        state_reaches(&mut state, "scene switch", |s| {
            s.scene.as_deref() == Some("BRB")
        })
        .await;
        fake.event(
            "SceneItemEnableStateChanged",
            json!({"sceneName": "Live", "sceneUuid": LIVE, "sceneItemId": 1, "sceneItemEnabled": false}),
        );
        state_reaches(&mut state, "item hidden", |s| !s.scenes[0].items[1].enabled).await;
        fake.event(
            "SceneItemLockStateChanged",
            json!({"sceneName": "Live", "sceneUuid": LIVE, "sceneItemId": 2, "sceneItemLocked": true}),
        );
        state_reaches(&mut state, "item locked", |s| s.scenes[0].items[0].locked).await;
        fake.event(
            "InputMuteStateChanged",
            json!({"inputName": "Mic", "inputUuid": MIC, "inputMuted": true}),
        );
        state_reaches(&mut state, "input muted", |s| s.inputs[0].muted).await;
        fake.event(
            "StreamStateChanged",
            json!({"outputActive": true, "outputState": "OBS_WEBSOCKET_OUTPUT_STARTED"}),
        );
        state_reaches(&mut state, "streaming", |s| s.streaming).await;

        // A structural change reads everything again (the fake's state is back to its start).
        let lists = *fake.scene_lists.lock().unwrap();
        fake.event(
            "SceneCreated",
            json!({"sceneName": "New", "sceneUuid": LIVE, "isGroup": false}),
        );
        state_reaches(&mut state, "read again", |s| {
            s.scene.as_deref() == Some("Live")
        })
        .await;
        assert_eq!(*fake.scene_lists.lock().unwrap(), lists + 1);

        fake.exit.notify_one();
        state_reaches(&mut state, "reset on disconnect", |s| {
            *s == StreamingState::default()
        })
        .await;
        // Connecting again reads the state again.
        state_reaches(&mut state, "read on reconnect", |s| s.scene.is_some()).await;
    }

    #[tokio::test]
    async fn commands_go_through_the_current_connection() {
        let fake = FakeObs::default();
        let (mut status, _, control) = run_with_state(start(fake.clone()).await, None);
        reaches(&mut status, ObsStatus::CONNECTED).await;

        let sources = control.browser_sources().await.unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(
            (sources[0].name.as_str(), sources[0].url.as_str()),
            ("Chat", "http://127.0.0.1:47810/overlay/chat/")
        );
        control.switch_scene("BRB").await.unwrap();
        control.set_item_enabled("Live", 1, false).await.unwrap();
        control.set_item_locked("Live", 2, true).await.unwrap();
        control
            .create_browser_source(
                "Live",
                "Chat",
                "http://127.0.0.1:47810/overlay/chat/",
                (400, 600),
            )
            .await
            .unwrap();
        control.add_source("BRB", "Chat").await.unwrap();
        control.remove_item("Live", 9).await.unwrap();
        // Front to back, position 0 of the two items is OBS's index 1.
        control.set_item_position("Live", 2, 0).await.unwrap();
        let (canvas, placements) = control.placements("Live").await.unwrap();
        assert_eq!((canvas.width, canvas.height), (1920, 1080));
        assert_eq!(
            placements[0],
            (
                1,
                Placement {
                    x: 40.0,
                    y: 30.0,
                    align: Align::TopLeft
                },
                ItemSize {
                    width: 400.0,
                    height: 600.0
                }
            )
        );
        control
            .set_placement(
                "Live",
                2,
                Placement {
                    x: 960.0,
                    y: 540.0,
                    align: Align::Center,
                },
            )
            .await
            .unwrap();
        // The source is 400 × 600: half of it is a scale of 0.5.
        control
            .set_item_size(
                "Live",
                2,
                ItemSize {
                    width: 200.0,
                    height: 300.0,
                },
            )
            .await
            .unwrap();
        let t = control.item_transform("Live", 2).await.unwrap();
        assert_eq!((t.rotation, t.flip_h, t.bounds), (0.0, false, Bounds::None));
        // A flip turns the scale's sign; a bounding box starts at the item's size now.
        control
            .set_item_transform(
                "Live",
                2,
                TransformChange {
                    rotation: Some(90.0),
                    flip_h: Some(true),
                    bounds: Some(Bounds::ScaleInner),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        // Only the choices OBS offers as enabled.
        assert_eq!(
            control
                .input_list_items("Cam", "video_device_id")
                .await
                .unwrap(),
            vec![("Cam".to_owned(), json!("cam1"))]
        );
        let (kind, settings) = control.input_settings("Chat").await.unwrap();
        assert_eq!(
            (kind.as_str(), &settings["width"]),
            ("browser_source", &json!(800))
        );
        // A field never changed comes from the kind's defaults.
        assert_eq!(settings["height"], json!(600));
        let mut change = serde_json::Map::new();
        change.insert("css".to_owned(), json!("body { color: red; }"));
        control.set_input_settings("Chat", change).await.unwrap();
        control.set_studio_mode(true).await.unwrap();
        control.set_preview("BRB").await.unwrap();
        control.transition(Some("Cut")).await.unwrap();
        assert_eq!(
            *fake.commands.lock().unwrap(),
            vec![
                (
                    "SetCurrentProgramScene".to_owned(),
                    json!({"sceneName": "BRB"})
                ),
                (
                    "SetSceneItemEnabled".to_owned(),
                    json!({"sceneName": "Live", "sceneItemId": 1, "sceneItemEnabled": false})
                ),
                (
                    "SetSceneItemLocked".to_owned(),
                    json!({"sceneName": "Live", "sceneItemId": 2, "sceneItemLocked": true})
                ),
                (
                    "CreateInput".to_owned(),
                    json!({"sceneName": "Live", "inputName": "Chat", "inputKind": "browser_source",
                           "inputSettings": {"url": "http://127.0.0.1:47810/overlay/chat/", "width": 400, "height": 600},
                           "sceneItemEnabled": true})
                ),
                (
                    "CreateSceneItem".to_owned(),
                    json!({"sceneName": "BRB", "sourceName": "Chat", "sceneItemEnabled": true})
                ),
                (
                    "RemoveSceneItem".to_owned(),
                    json!({"sceneName": "Live", "sceneItemId": 9})
                ),
                (
                    "SetSceneItemIndex".to_owned(),
                    json!({"sceneName": "Live", "sceneItemId": 2, "sceneItemIndex": 1})
                ),
                (
                    "SetSceneItemTransform".to_owned(),
                    json!({"sceneName": "Live", "sceneItemId": 2, "sceneItemTransform": {
                        "positionX": 960.0, "positionY": 540.0, "alignment": 0}})
                ),
                (
                    "SetSceneItemTransform".to_owned(),
                    json!({"sceneName": "Live", "sceneItemId": 2, "sceneItemTransform": {
                        "scaleX": 0.5, "scaleY": 0.5}})
                ),
                (
                    "SetSceneItemTransform".to_owned(),
                    json!({"sceneName": "Live", "sceneItemId": 2, "sceneItemTransform": {
                        "rotation": 90.0, "scaleX": -1.0, "boundsType": "OBS_BOUNDS_SCALE_INNER",
                        "boundsWidth": 400.0, "boundsHeight": 600.0}})
                ),
                (
                    "SetInputSettings".to_owned(),
                    json!({"inputName": "Chat", "inputSettings": {"css": "body { color: red; }"}, "overlay": true})
                ),
                (
                    "SetStudioModeEnabled".to_owned(),
                    json!({"studioModeEnabled": true})
                ),
                (
                    "SetCurrentPreviewScene".to_owned(),
                    json!({"sceneName": "BRB"})
                ),
                (
                    "SetCurrentSceneTransition".to_owned(),
                    json!({"transitionName": "Cut"})
                ),
            ]
        );
        assert_eq!(
            control.switch_scene("Missing").await,
            Err(StreamingError::NotFound)
        );

        // Once OBS closes, commands report not connected instead of waiting for it.
        fake.exit.notify_one();
        reaches(
            &mut status,
            ObsStatus::reconnecting(Some("obs.disconnected")),
        )
        .await;
        assert_eq!(
            control.switch_scene("Live").await,
            Err(StreamingError::NotConnected)
        );
    }

    async fn reaches(rx: &mut watch::Receiver<ObsStatus>, expected: ObsStatus) {
        let reached = tokio::time::timeout(Duration::from_secs(5), rx.wait_for(|s| *s == expected))
            .await
            .is_ok();
        assert!(
            reached,
            "status stayed {:?}, expected {expected:?}",
            *rx.borrow()
        );
    }

    #[tokio::test]
    async fn status_follows_obs_closing_and_coming_back() {
        let fake = FakeObs::default();
        let mut rx = run(start(fake.clone()).await, None);
        reaches(&mut rx, ObsStatus::CONNECTED).await;

        fake.exit.notify_one();
        reaches(&mut rx, ObsStatus::reconnecting(Some("obs.disconnected"))).await;
        // The first retry comes after 1 s.
        reaches(&mut rx, ObsStatus::CONNECTED).await;
        assert_eq!(fake.identify.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn stalled_handshake_times_out_and_retries() {
        let fake = FakeObs {
            stall: true,
            ..FakeObs::default()
        };
        let mut rx = run(start(fake.clone()).await, None);
        reaches(
            &mut rx,
            ObsStatus::reconnecting(Some("obs.handshake_failed")),
        )
        .await;
        // The 1 s backoff, then another attempt that stalls again.
        let retried = async {
            while fake.identify.lock().unwrap().len() < 2 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        };
        assert!(
            tokio::time::timeout(Duration::from_secs(5), retried)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn closed_obs_is_reported_as_reconnecting() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let mut rx = run(port, None);
        reaches(&mut rx, ObsStatus::reconnecting(Some("obs.unreachable"))).await;
    }

    #[tokio::test]
    async fn wrong_password_is_an_error_with_its_reason() {
        let fake = FakeObs {
            require_auth: true,
            ..FakeObs::default()
        };
        let mut rx = run(start(fake.clone()).await, Some("hunter2"));
        reaches(&mut rx, ObsStatus::error("obs.auth_failed")).await;
        let identify = fake.identify.lock().unwrap()[0].to_string();
        assert!(identify.contains("authentication"));
        assert!(
            !identify.contains("hunter2"),
            "only the challenge response is sent"
        );
    }

    #[tokio::test]
    async fn missing_password_is_its_own_reason() {
        let fake = FakeObs {
            require_auth: true,
            ..FakeObs::default()
        };
        let mut rx = run(start(fake).await, None);
        reaches(&mut rx, ObsStatus::error("obs.password_required")).await;
    }

    #[test]
    fn alignment_round_trips_through_obs_flags() {
        for align in [
            Align::TopLeft,
            Align::Top,
            Align::TopRight,
            Align::Left,
            Align::Center,
            Align::Right,
            Align::BottomLeft,
            Align::Bottom,
            Align::BottomRight,
        ] {
            assert_eq!(align_of(alignment_of(align)), align);
        }
    }

    #[test]
    fn the_size_on_the_canvas_leaves_out_the_crop_and_the_flip_or_is_the_bounding_box() {
        let t = |extra: Value| {
            let mut v = json!({"sourceWidth": 1920.0, "sourceHeight": 1080.0, "positionX": 0.0, "positionY": 0.0,
                "rotation": 0.0, "scaleX": -0.5, "scaleY": 0.5, "width": -960.0, "height": 540.0, "alignment": 5,
                "boundsType": "OBS_BOUNDS_NONE", "boundsAlignment": 0, "boundsWidth": 0.0, "boundsHeight": 0.0,
                "cropLeft": 100, "cropRight": 100, "cropTop": 0, "cropBottom": 80});
            v.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            serde_json::from_value::<obws::responses::scene_items::SceneItemTransform>(v).unwrap()
        };
        assert_eq!(
            shown_size(&t(json!({}))),
            ItemSize {
                width: 860.0,
                height: 500.0
            }
        );
        assert_eq!(
            shown_size(&t(
                json!({"boundsType": "OBS_BOUNDS_STRETCH", "boundsWidth": 300.0, "boundsHeight": 200.0})
            )),
            ItemSize {
                width: 300.0,
                height: 200.0
            }
        );
    }

    #[test]
    fn timeout_means_unreachable() {
        assert_eq!(
            classify(&ObwsError::Timeout, true),
            ObsStatus::reconnecting(Some("obs.unreachable"))
        );
    }

    #[test]
    fn status_serializes_for_the_status_light() {
        assert_eq!(
            serde_json::to_value(ObsStatus::error("obs.auth_failed")).unwrap(),
            json!({"state": "error", "reason": "obs.auth_failed"})
        );
    }
}
