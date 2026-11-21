//! Test values for the counter and marathon timer overlays, from the 模擬事件 dialog: the overlay shows a fixed test
//! value for a few seconds and then its real one again. The events are tagged `source=simulation` and never kept in
//! snapshots, and nothing goes through the counters themselves, so no real number or save changes. Admin route; the
//! caller adds the token check.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use serde::{Deserialize, Serialize};
use toolkit_core::{
    bus::{Event, EventBus},
    events::Source,
    modules::{self, Modules},
};
use utoipa::ToSchema;

use crate::admin::{ErrorBody, error};

const COUNTER_TEST: &str = "counter.test";
const MARATHON_TEST: &str = "marathon.test";
const VERSION: u32 = 1;
/// The value the counter overlay shows as its test row.
const COUNTER_TEST_VALUE: i64 = 99;
/// The time the marathon overlay shows while testing (30 minutes, not counting down).
const MARATHON_TEST_MS: u64 = 30 * 60 * 1000;

#[derive(Clone)]
pub(crate) struct OverlayTestState {
    bus: Arc<EventBus>,
    modules: Arc<Modules>,
}

pub(crate) fn routes(bus: Arc<EventBus>, modules: Arc<Modules>) -> Router {
    Router::new()
        .route("/api/admin/simulate/overlay-test", post(test_overlay))
        .with_state(OverlayTestState { bus, modules })
}

/// Which overlay gets a test value.
#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum TestOverlay {
    Counter,
    Marathon,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct OverlayTest {
    overlay: TestOverlay,
}

/// A `counter.test` event exactly as `/ws/overlay` sends it: the counter overlay shows an extra test row for a few
/// seconds. Not kept in snapshots.
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CounterTestEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: CounterTestType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: CounterTestPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum CounterTestType {
    #[serde(rename = "counter.test")]
    CounterTest,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub(crate) struct CounterTestPayload {
    value: i64,
}

/// A `marathon.test` event exactly as `/ws/overlay` sends it: the marathon overlay shows a fixed time for a few
/// seconds. Not kept in snapshots.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MarathonTestEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: MarathonTestType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: MarathonTestPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum MarathonTestType {
    #[serde(rename = "marathon.test")]
    MarathonTest,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MarathonTestPayload {
    remaining_ms: u64,
}

/// Shows a fixed test value on the counter or marathon timer overlay for a few seconds (`counter.test`,
/// `marathon.test`); no counter or timer changes.
#[utoipa::path(
    post,
    path = "/api/admin/simulate/overlay-test",
    tag = "admin",
    request_body = OverlayTest,
    responses(
        (status = 204),
        (status = 400, description = "`simulate.invalid_overlay`", body = ErrorBody),
        (status = 409, description = "`simulate.feature_off`: counters are turned off", body = ErrorBody),
    )
)]
pub(crate) async fn test_overlay(
    State(s): State<OverlayTestState>,
    body: Result<Json<OverlayTest>, JsonRejection>,
) -> Response {
    let Ok(Json(OverlayTest { overlay })) = body else {
        return error(StatusCode::BAD_REQUEST, "simulate.invalid_overlay");
    };
    // The marathon timer belongs to the counter module.
    if !s.modules.is_enabled(modules::COUNTER) {
        return error(StatusCode::CONFLICT, "simulate.feature_off");
    }
    let (event_type, payload) = match overlay {
        TestOverlay::Counter => (
            COUNTER_TEST,
            serde_json::json!({ "value": COUNTER_TEST_VALUE }),
        ),
        TestOverlay::Marathon => (
            MARATHON_TEST,
            serde_json::json!({ "remainingMs": MARATHON_TEST_MS }),
        ),
    };
    s.bus.publish(Event {
        id: uuid::Uuid::now_v7().to_string(),
        event_type,
        version: VERSION,
        occurred_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        source: Source::Simulation,
        payload,
    });
    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn post_body(body: &str) -> Request<Body> {
        Request::post("/api/admin/simulate/overlay-test")
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap()
    }

    #[tokio::test]
    async fn test_values_reach_the_overlays_as_simulation_and_are_never_replayed() {
        let dir = std::env::temp_dir().join(format!("st-overlay-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bus = Arc::new(EventBus::default());
        let modules = Arc::new(Modules::load(dir.clone()));
        modules.set_enabled(modules::COUNTER, true).unwrap();
        let app = routes(bus.clone(), modules.clone());
        let (_, mut rx) = bus.subscribe();

        let res = app
            .clone()
            .oneshot(post_body(r#"{"overlay":"counter"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let event: serde_json::Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(
            (event["type"].as_str(), event["source"].as_str()),
            (Some("counter.test"), Some("simulation"))
        );
        assert_eq!(event["payload"], serde_json::json!({ "value": 99 }));
        let typed: CounterTestEvent = serde_json::from_value(event.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(typed).unwrap(),
            event,
            "the schema type matches the bus"
        );

        let res = app
            .clone()
            .oneshot(post_body(r#"{"overlay":"marathon"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let event: serde_json::Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(event["type"], "marathon.test");
        assert_eq!(
            event["payload"],
            serde_json::json!({ "remainingMs": 1_800_000 })
        );
        let typed: MarathonTestEvent = serde_json::from_value(event.clone()).unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap(), event);

        let (snapshot, _) = bus.subscribe();
        assert!(
            !snapshot.contains(".test"),
            "never replayed to an overlay that connects later"
        );

        let res = app
            .clone()
            .oneshot(post_body(r#"{"overlay":"bgm"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        modules.set_enabled(modules::COUNTER, false).unwrap();
        let res = app
            .oneshot(post_body(r#"{"overlay":"counter"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);
        assert!(
            rx.try_recv().is_err(),
            "nothing sent while counters are off"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
