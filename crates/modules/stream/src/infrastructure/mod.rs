//! Publishes the live state as the `stream.status` event, kept in the WebSocket snapshot.

use std::sync::Arc;

use chrono::SecondsFormat;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use toolkit_core::{
    bus::{Event, EventBus},
    events::Source,
};
use utoipa::{OpenApi, ToSchema};

use crate::application::{Broadcast, StreamService};

pub mod db;

const STATUS: &str = "stream.status";
const STATUS_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StreamStateName {
    /// Nothing from Twitch yet (signed out, or the first check has not answered).
    Unknown,
    Offline,
    Live,
}

/// The `payload` of a `stream.status` event.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamStatusPayload {
    pub state: StreamStateName,
    /// While live: Twitch's start time (RFC 3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    /// While live, once a check has counted them (every minute).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewers: Option<u32>,
}

/// A `stream.status` event exactly as `/ws/overlay` sends it; the snapshot keeps only the latest one.
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamStatusEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: StreamStatusType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: StreamStatusPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum StreamStatusType {
    #[serde(rename = "stream.status")]
    StreamStatus,
}

fn payload(live: &Broadcast) -> StreamStatusPayload {
    match live {
        Broadcast::Unknown => StreamStatusPayload {
            state: StreamStateName::Unknown,
            started_at: None,
            viewers: None,
        },
        Broadcast::Offline => StreamStatusPayload {
            state: StreamStateName::Offline,
            started_at: None,
            viewers: None,
        },
        Broadcast::Live {
            started_at,
            viewers,
        } => StreamStatusPayload {
            state: StreamStateName::Live,
            started_at: Some(started_at.to_rfc3339_opts(SecondsFormat::Millis, true)),
            viewers: *viewers,
        },
    }
}

/// Starts stream sessions in `pool`, which must already have [`db::MIGRATOR`] applied, publishing the live state
/// on `bus`; needs a Tokio runtime.
pub fn start(pool: SqlitePool, bus: Arc<EventBus>) -> StreamService {
    bus.retain(STATUS, 1);
    StreamService::start(db::SqliteSessions::new(pool), move |live| {
        bus.publish(Event {
            id: uuid::Uuid::now_v7().to_string(),
            event_type: STATUS,
            version: STATUS_VERSION,
            occurred_at: chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            source: Source::Twitch,
            payload: serde_json::to_value(payload(live)).expect("status serializes"),
        });
    })
}

#[derive(OpenApi)]
#[openapi(components(schemas(StreamStatusEvent)))]
struct StreamApi;

/// This module's part of the Host's OpenAPI document.
pub fn openapi() -> utoipa::openapi::OpenApi {
    StreamApi::openapi()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use toolkit_core::events::{LiveStream, StreamChange};

    async fn next_status(rx: &mut tokio::sync::broadcast::Receiver<Arc<str>>) -> serde_json::Value {
        let text = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[tokio::test]
    async fn the_live_state_follows_starts_checks_and_ends_and_sessions_are_stored() {
        let pool = db::tests::pool().await;
        let bus = Arc::new(EventBus::default());
        let (_, mut rx) = bus.subscribe();
        let service = start(pool.clone(), bus.clone());
        assert_eq!(
            next_status(&mut rx).await["payload"],
            serde_json::json!({"state": "unknown"})
        );

        // Started mid-stream: a check finds it, with Twitch's start time and the viewers.
        let checked = |viewers| {
            StreamChange::Checked(Some(LiveStream {
                id: "9001".into(),
                started_at: "2026-11-19T10:00:00Z".into(),
                viewers,
            }))
        };
        service.receive(checked(42));
        let live = next_status(&mut rx).await;
        assert_eq!(
            live["payload"],
            serde_json::json!({"state": "live", "startedAt": "2026-11-19T10:00:00.000Z", "viewers": 42})
        );
        let typed: StreamStatusEvent = serde_json::from_value(live.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(typed).unwrap(),
            live,
            "the schema type matches the bus"
        );

        // The same stream's start event, then another check: still one session, new viewer count.
        service.receive(StreamChange::Online {
            id: "9001".into(),
            started_at: "2026-11-19T10:00:00Z".into(),
        });
        next_status(&mut rx).await;
        service.receive(checked(50));
        assert_eq!(next_status(&mut rx).await["payload"]["viewers"], 50);

        // Signed out mid-stream: unknown, not live; signed in again, the next check says live.
        service.receive(StreamChange::SignedOut);
        assert_eq!(
            next_status(&mut rx).await["payload"],
            serde_json::json!({"state": "unknown"})
        );
        service.receive(checked(51));
        assert_eq!(next_status(&mut rx).await["payload"]["state"], "live");

        service.receive(StreamChange::Offline);
        assert_eq!(
            next_status(&mut rx).await["payload"],
            serde_json::json!({"state": "offline"})
        );
        // A late check still listing the ended stream does not bring it back.
        service.receive(checked(50));
        assert_eq!(next_status(&mut rx).await["payload"]["state"], "offline");

        let rows: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT twitch_stream_id, ended_at FROM stream_sessions")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(rows.len(), 1, "one session per stream");
        assert!(
            rows[0].1.is_some(),
            "kept, with its end, until the next stream starts"
        );
        // Checks while live counted 42, 50 and 51.
        assert_eq!(
            db::latest_session(&pool)
                .await
                .unwrap()
                .unwrap()
                .peak_viewers,
            Some(51)
        );

        // A restart reads the last session back; the next stream opens a new one.
        let (_, mut rx) = bus.subscribe();
        let restarted = start(pool.clone(), bus.clone());
        next_status(&mut rx).await;
        restarted.receive(StreamChange::Online {
            id: "9002".into(),
            started_at: "2026-11-20T10:00:00Z".into(),
        });
        // Asked right after the start, before the worker has stored it: the answer is that stream's session.
        let answered = restarted.session_after_pending().await;
        let id: String =
            sqlx::query_scalar("SELECT id FROM stream_sessions WHERE twitch_stream_id = '9002'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(answered, Some(id));
        assert_eq!(next_status(&mut rx).await["payload"]["state"], "live");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM stream_sessions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 2);
    }
}
