//! The presentation queue for the admin UI: its status as the `presentation.queue` event (kept in the WebSocket
//! snapshot), and clearing it or stopping everything. Admin routes; the caller adds the token check.

use std::sync::Arc;

use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};
use toolkit_core::{
    bus::{Event, EventBus},
    events::Source,
    presentation::{Presenter, QueueStatus},
};
use utoipa::ToSchema;

const QUEUE_EVENT: &str = "presentation.queue";
const STOPPED_EVENT: &str = "presentation.stopped";
const VERSION: u32 = 1;

/// A `presentation.queue` event exactly as `/ws/overlay` sends it; the snapshot keeps only the latest one.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PresentationQueueEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: PresentationQueueType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: QueueStatus,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum PresentationQueueType {
    #[serde(rename = "presentation.queue")]
    PresentationQueue,
}

/// A `presentation.stopped` event exactly as `/ws/overlay` sends it: every overlay stops its picture and sound at
/// once. Not kept in snapshots.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PresentationStoppedEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: PresentationStoppedType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: Stopped,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum PresentationStoppedType {
    #[serde(rename = "presentation.stopped")]
    PresentationStopped,
}

fn publish(bus: &EventBus, event_type: &'static str, payload: serde_json::Value) {
    bus.publish(Event {
        id: uuid::Uuid::now_v7().to_string(),
        event_type,
        version: VERSION,
        occurred_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        source: Source::System,
        payload,
    });
}

/// Keeps the latest queue status in snapshots.
pub(crate) fn retain(bus: &EventBus) {
    bus.retain(QUEUE_EVENT, 1);
}

pub(crate) fn publish_status(bus: &EventBus, status: &QueueStatus) {
    publish(
        bus,
        QUEUE_EVENT,
        serde_json::to_value(status).expect("status serializes"),
    );
}

/// Keeps the latest queue status in snapshots and publishes every change.
pub(crate) fn follow(presenter: &Presenter, bus: Arc<EventBus>) {
    retain(&bus);
    let mut status = presenter.subscribe();
    tokio::spawn(async move {
        loop {
            let current = status.borrow_and_update().clone();
            publish_status(&bus, &current);
            if status.changed().await.is_err() {
                return;
            }
        }
    });
}

#[derive(Clone)]
pub(crate) struct QueueState {
    presenter: Arc<Presenter>,
    bus: Arc<EventBus>,
}

pub(crate) fn routes(presenter: Arc<Presenter>, bus: Arc<EventBus>) -> Router {
    Router::new()
        .route("/api/admin/queue/clear", post(clear_queue))
        .route("/api/admin/queue/stop", post(stop_queue))
        .with_state(QueueState { presenter, bus })
}

#[derive(Debug, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub(crate) struct Cleared {
    /// Waiting items that will not play.
    cancelled: usize,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub(crate) struct Stopped {
    /// Whether an item was playing.
    stopped: bool,
    /// Waiting items that will not play.
    cancelled: usize,
}

/// Cancels every waiting item; the one playing finishes. Records (redemptions, check-ins) are not touched.
#[utoipa::path(
    post,
    path = "/api/admin/queue/clear",
    tag = "admin",
    responses((status = 200, body = Cleared))
)]
pub(crate) async fn clear_queue(State(s): State<QueueState>) -> Json<Cleared> {
    Json(Cleared {
        cancelled: s.presenter.clear(),
    })
}

/// Emergency stop: ends the item playing and cancels the waiting ones; overlays stop picture and sound at once.
/// Records (redemptions, check-ins) are not touched.
#[utoipa::path(
    post,
    path = "/api/admin/queue/stop",
    tag = "admin",
    responses((status = 200, body = Stopped))
)]
pub(crate) async fn stop_queue(State(s): State<QueueState>) -> Json<Stopped> {
    // Published by stop() itself, under the queue's lock, so nothing queued after it plays before overlays hear it.
    let bus = s.bus.clone();
    let (stopped, cancelled) = s.presenter.stop(move |stopped, cancelled| {
        publish(
            &bus,
            STOPPED_EVENT,
            serde_json::to_value(Stopped { stopped, cancelled }).expect("serializes"),
        )
    });
    let result = Stopped { stopped, cancelled };
    Json(result)
}
