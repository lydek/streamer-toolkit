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
    /// The scene's own items; items inside a group are not listed.
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Input {
    pub name: String,
    pub muted: bool,
}

/// The streaming software as features see it. Commands (switch scene, show an item, mute) join when a feature
/// needs them.
pub trait StreamingSoftware: Send + Sync {
    /// The latest state.
    fn state(&self) -> StreamingState;
}
