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
}
