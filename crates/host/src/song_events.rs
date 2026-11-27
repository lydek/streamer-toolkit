//! 點歌's notice on the bus (docs/modules/songs.md): the queue changed, so the 點歌 page reads it again.

use std::sync::Arc;

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use toolkit_core::{
    bus::{Event, EventBus},
    events::Source,
};
use utoipa::ToSchema;

pub(crate) const CHANGED: &str = "songs.changed";

/// A `songs.changed` event exactly as `/ws/overlay` sends it: the queue changed (a request from chat or a change on
/// the page). It carries nothing and is not kept in the snapshot; the page reads `GET /api/admin/song-requests`.
// The bus serializes its own envelope.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SongsChangedEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: SongsChangedType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: SongsChangedPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum SongsChangedType {
    #[serde(rename = "songs.changed")]
    SongsChanged,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub(crate) struct SongsChangedPayload {}

/// Announces each change of the queue on the bus.
pub(crate) fn announce_changes(bus: Arc<EventBus>) -> songs::Changed {
    Arc::new(move || {
        bus.publish(Event {
            id: uuid::Uuid::now_v7().to_string(),
            event_type: CHANGED,
            version: 1,
            occurred_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            source: Source::System,
            payload: serde_json::json!({}),
        });
    })
}
