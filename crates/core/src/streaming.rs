//! The streaming-software port: scenes, sources and outputs, whatever program provides them (OBS today). Features
//! such as scenes and modes read this instead of an OBS SDK; the adapter lives in `integrations`.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// What the streaming software shows right now; [`StreamingState::default`] while it is not connected.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamingState {
    /// The scene on air; `null` while not connected (unknown).
    #[schema(required)]
    pub scene: Option<String>,
    /// In the program's own order.
    pub scenes: Vec<Scene>,
    pub inputs: Vec<Input>,
    pub streaming: bool,
    pub recording: bool,
    /// Whether the program's studio mode is on: picking a scene puts it in the preview, and a transition puts the
    /// preview on air.
    #[serde(default)]
    pub studio_mode: bool,
    /// The scene in the preview while studio mode is on; `null` otherwise.
    #[serde(default)]
    #[schema(required)]
    pub preview: Option<String>,
    /// The transitions to pick from, in the program's order.
    #[serde(default)]
    pub transitions: Vec<String>,
    /// The transition in use.
    #[serde(default)]
    #[schema(required)]
    pub transition: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    pub name: String,
    /// The scene's own items, front to back (as the program's source list shows them); items inside a group are
    /// not listed.
    pub items: Vec<SceneItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SceneItem {
    /// Unique within its scene.
    pub id: i64,
    /// The source the item shows.
    pub source: String,
    /// Whether the item is shown.
    pub enabled: bool,
    /// Whether the item is locked against moving it by mistake in the program.
    pub locked: bool,
    /// What the source is: the program's input kind (OBS: `browser_source`, `game_capture`…), `scene` for a nested
    /// scene, `group` for a group; empty when the program does not say.
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Input {
    pub name: String,
    pub muted: bool,
}

/// Which point of an item its position is: a corner, the middle of an edge, or the center.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

/// Where an item sits on the canvas: the canvas pixel its `align` point is at.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub align: Align,
}

/// An item's size on the canvas: its source (less what is cropped) times its scale, in canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ItemSize {
    pub width: f32,
    pub height: f32,
}

/// How an item is turned, flipped, cropped and fitted, besides where it sits and how large it is.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ItemTransform {
    /// Degrees clockwise around its align point, 0-360.
    pub rotation: f32,
    pub flip_h: bool,
    pub flip_v: bool,
    /// Source pixels cut off each side before scaling.
    pub crop: ItemCrop,
    pub bounds: Bounds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ToSchema)]
pub struct ItemCrop {
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
    pub left: u32,
}

/// How an item fills its bounding box: `none` scales its content by its size; the rest make its size the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Bounds {
    None,
    Stretch,
    ScaleInner,
    ScaleOuter,
    ScaleToWidth,
    ScaleToHeight,
    MaxOnly,
}

/// A transform change: only the parts given change; the rest of the item's transform stays.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, ToSchema)]
pub struct TransformChange {
    #[serde(default)]
    pub rotation: Option<f32>,
    #[serde(default)]
    pub flip_h: Option<bool>,
    #[serde(default)]
    pub flip_v: Option<bool>,
    #[serde(default)]
    pub crop: Option<ItemCrop>,
    #[serde(default)]
    pub bounds: Option<Bounds>,
}

/// The canvas size in pixels (OBS: the base resolution).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
}

/// A browser source and the address it opens. The address may hold another service's token, so it never reaches
/// the admin UI or a log as is: the Host keeps only the paths of its own overlays.
#[derive(Clone, PartialEq, Eq)]
pub struct BrowserSource {
    pub name: String,
    pub url: String,
}

/// Why a command did not reach the streaming software.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamingError {
    /// Not connected right now.
    NotConnected,
    /// No such scene or item.
    NotFound,
    /// The program turned the request down or the connection failed while sending it.
    Failed,
}

/// The streaming software as features see it. Commands join when a feature needs them; the state follows the
/// program's own events, so a command's effect shows up there rather than in its result.
/// Counters the streaming software reports. Frame counts are totals since it started (or the stream did), so rates
/// come from two readings.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StreamingStats {
    /// The streaming software's own CPU use, in percent.
    pub cpu_percent: f64,
    /// Frames the renderer skipped (too slow), of all it rendered.
    pub render_skipped: u32,
    pub render_total: u32,
    pub streaming: bool,
    /// Frames the stream output dropped (network), of all it sent; 0 while not streaming.
    pub stream_skipped: u32,
    pub stream_total: u32,
    /// Bytes the stream output sent.
    pub stream_bytes: u64,
}

pub trait StreamingSoftware: Send + Sync {
    /// The latest state.
    fn state(&self) -> StreamingState;
    /// Puts `scene` on air.
    fn switch_scene(&self, scene: &str) -> impl Future<Output = Result<(), StreamingError>> + Send;
    /// Shows or hides item `item` of `scene`.
    fn set_item_enabled(
        &self,
        scene: &str,
        item: i64,
        enabled: bool,
    ) -> impl Future<Output = Result<(), StreamingError>> + Send;
    /// Locks or unlocks item `item` of `scene`.
    fn set_item_locked(
        &self,
        scene: &str,
        item: i64,
        locked: bool,
    ) -> impl Future<Output = Result<(), StreamingError>> + Send;
    /// Its counters now.
    fn stats(&self) -> impl Future<Output = Result<StreamingStats, StreamingError>> + Send;
    /// Every browser source with the address it opens.
    fn browser_sources(
        &self,
    ) -> impl Future<Output = Result<Vec<BrowserSource>, StreamingError>> + Send;
    /// Creates browser source `name` opening `url` at `width` × `height`, as a new item of `scene`.
    fn create_browser_source(
        &self,
        scene: &str,
        name: &str,
        url: &str,
        size: (u32, u32),
    ) -> impl Future<Output = Result<(), StreamingError>> + Send;
    /// Adds the existing source `source` to `scene` as a new item.
    fn add_source(
        &self,
        scene: &str,
        source: &str,
    ) -> impl Future<Output = Result<(), StreamingError>> + Send;
    /// Moves item `item` of `scene` to `position`, counted front to back (0 is the front), as `items` lists them.
    fn set_item_position(
        &self,
        scene: &str,
        item: i64,
        position: u32,
    ) -> impl Future<Output = Result<(), StreamingError>> + Send;
    /// The canvas size, and where each of `scene`'s items sits and how large it is, by item ID.
    #[allow(clippy::type_complexity)]
    fn placements(
        &self,
        scene: &str,
    ) -> impl Future<Output = Result<(Canvas, Vec<(i64, Placement, ItemSize)>), StreamingError>> + Send;
    /// Scales item `item` of `scene` to `size`; position, rotation and crop stay. `NotFound` for an item with
    /// nothing to scale (sound only).
    fn set_item_size(
        &self,
        scene: &str,
        item: i64,
        size: ItemSize,
    ) -> impl Future<Output = Result<(), StreamingError>> + Send;
    /// Moves item `item` of `scene` to `placement`.
    fn set_placement(
        &self,
        scene: &str,
        item: i64,
        placement: Placement,
    ) -> impl Future<Output = Result<(), StreamingError>> + Send;
    /// How item `item` of `scene` is turned, flipped, cropped and fitted.
    fn item_transform(
        &self,
        scene: &str,
        item: i64,
    ) -> impl Future<Output = Result<ItemTransform, StreamingError>> + Send;
    /// Changes the parts of item `item`'s transform that `change` gives; position and size stay.
    fn set_item_transform(
        &self,
        scene: &str,
        item: i64,
        change: TransformChange,
    ) -> impl Future<Output = Result<(), StreamingError>> + Send;
    /// Input `input`'s kind and its settings as the program stores them (callers pick the fields they show).
    fn input_settings(
        &self,
        input: &str,
    ) -> impl Future<
        Output = Result<(String, serde_json::Map<String, serde_json::Value>), StreamingError>,
    > + Send;
    /// Merges `settings` into input `input`'s settings; fields not given stay.
    fn set_input_settings(
        &self,
        input: &str,
        settings: serde_json::Map<String, serde_json::Value>,
    ) -> impl Future<Output = Result<(), StreamingError>> + Send;
    /// Turns studio mode on or off.
    fn set_studio_mode(
        &self,
        enabled: bool,
    ) -> impl Future<Output = Result<(), StreamingError>> + Send;
    /// Puts `scene` in the preview (studio mode).
    fn set_preview(&self, scene: &str) -> impl Future<Output = Result<(), StreamingError>> + Send;
    /// Puts the preview on air with `transition`, or the one in use when `None` (studio mode).
    fn transition(
        &self,
        transition: Option<&str>,
    ) -> impl Future<Output = Result<(), StreamingError>> + Send;
    /// Removes item `item` from `scene`; the source stays in the program's other scenes.
    fn remove_item(
        &self,
        scene: &str,
        item: i64,
    ) -> impl Future<Output = Result<(), StreamingError>> + Send;
}
