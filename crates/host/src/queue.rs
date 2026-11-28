//! The presentation queue for the admin UI: its status as the `presentation.queue` event (kept in the WebSocket
//! snapshot), and skipping the item playing, clearing it or stopping everything. Admin routes; the caller adds the
//! token check.

use std::sync::Arc;

use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};
use toolkit_core::time::{self, Timestamp};
use toolkit_core::{
    bus::{Event, EventBus},
    events::Source,
    presentation::{Presenter, QueueStatus, Stage},
};
use utoipa::ToSchema;

const QUEUE_EVENT: &str = "presentation.queue";
const STOPPED_EVENT: &str = "presentation.stopped";
const SKIPPED_EVENT: &str = "presentation.skipped";
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

/// A `presentation.skipped` event exactly as `/ws/overlay` sends it: the overlay of `stage` stops its picture and
/// sound at once. Not kept in snapshots.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PresentationSkippedEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: PresentationSkippedType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: SkippedPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum PresentationSkippedType {
    #[serde(rename = "presentation.skipped")]
    PresentationSkipped,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub(crate) struct SkippedPayload {
    /// The overlay whose item was skipped.
    stage: Stage,
}

fn publish(bus: &EventBus, event_type: &'static str, payload: serde_json::Value) {
    bus.publish(Event {
        id: uuid::Uuid::now_v7().to_string(),
        event_type,
        version: VERSION,
        occurred_at: time::millis(Timestamp::now()),
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
        .route("/api/admin/queue/skip", post(skip_queue))
        .route("/api/admin/queue/clear", post(clear_queue))
        .route("/api/admin/queue/stop", post(stop_queue))
        .with_state(QueueState { presenter, bus })
}

#[derive(Debug, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub(crate) struct Skipped {
    /// Whether an item was playing.
    skipped: bool,
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

/// Ends the item playing; its overlay stops picture and sound at once, and the waiting items follow after the gap.
/// Records (redemptions, check-ins) are not touched.
#[utoipa::path(
    post,
    path = "/api/admin/queue/skip",
    tag = "admin",
    responses((status = 200, body = Skipped))
)]
pub(crate) async fn skip_queue(State(s): State<QueueState>) -> Json<Skipped> {
    // Published by skip() itself, under the queue's lock, like the emergency stop.
    let bus = s.bus.clone();
    let skipped = s.presenter.skip(move |stage| {
        publish(
            &bus,
            SKIPPED_EVENT,
            serde_json::to_value(SkippedPayload { stage }).expect("serializes"),
        )
    });
    Json(Skipped { skipped })
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
    Json(stop_all(&s.presenter, &s.bus))
}

/// The emergency stop: ends the item playing and drops the waiting ones; every overlay hears it at once.
pub(crate) fn stop_all(presenter: &Presenter, bus: &Arc<EventBus>) -> Stopped {
    // Published by stop() itself, under the queue's lock, so nothing queued after it plays before overlays hear it.
    let bus = bus.clone();
    let (stopped, cancelled) = presenter.stop(move |stopped, cancelled| {
        publish(
            &bus,
            STOPPED_EVENT,
            serde_json::to_value(Stopped { stopped, cancelled }).expect("serializes"),
        )
    });
    Stopped { stopped, cancelled }
}
