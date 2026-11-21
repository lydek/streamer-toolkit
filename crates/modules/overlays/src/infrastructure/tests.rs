use super::*;
use axum::{body::Body, http::Request};
use serde_json::{Value, json};
use std::time::Duration;
use tower::ServiceExt;

struct Rig {
    pool: SqlitePool,
    bus: Arc<EventBus>,
    app: Router,
}

async fn rig() -> Rig {
    restart(db::tests::pool().await).await
}

/// The service again on the same data, as after restarting the app.
async fn restart(pool: SqlitePool) -> Rig {
    let bus = Arc::new(EventBus::default());
    let service = start(pool.clone(), bus.clone());
    // Let it announce the stored card.
    tokio::time::sleep(Duration::from_millis(50)).await;
    Rig {
        pool,
        bus,
        app: routes(service),
    }
}

async fn send(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// The latest `screen.break` in a new snapshot.
fn snapshot_break(bus: &EventBus) -> Value {
    let (snapshot, _) = bus.subscribe();
    let v: Value = serde_json::from_str(&snapshot).unwrap();
    v["payload"]["screen.break"][0].clone()
}

const BREAK_URL: &str = "/api/admin/screen-tools/break";

#[tokio::test]
async fn defaults_until_saved_and_settings_round_trip() {
    let r = rig().await;
    let (status, card) = send(&r.app, "GET", BREAK_URL, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(card["shown"], false);
    assert_eq!(card["endText"], "馬上回來");
    assert_eq!(card["minutes"], 10);
    assert_eq!(snapshot_break(&r.bus)["payload"]["shown"], false);

    let settings = json!({
        "text": " 準備中 ", "end_text": "就要開始", "mode": "until", "minutes": 5, "until_time": "20:30"
    });
    let (status, card) = send(&r.app, "PUT", BREAK_URL, Some(settings)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(card["text"], "準備中");
    assert_eq!(card["mode"], "until");
    assert_eq!(card["untilTime"], "20:30");
    assert_eq!(snapshot_break(&r.bus)["payload"]["text"], "準備中");

    let bad =
        json!({"text": "", "end_text": "", "mode": "minutes", "minutes": 0, "until_time": "20:30"});
    let (status, body) = send(&r.app, "PUT", BREAK_URL, Some(bad)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "overlays.invalid");
}

#[tokio::test]
async fn show_counts_down_by_minutes_or_until_a_time() {
    let r = rig().await;
    let before = Utc::now();
    let (status, card) = send(
        &r.app,
        "POST",
        &format!("{BREAK_URL}/show"),
        Some(json!({"minutes": 10})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(card["shown"], true);
    let ends = DateTime::parse_from_rfc3339(card["endsAt"].as_str().unwrap()).unwrap();
    let minutes = (ends.to_utc() - before).num_seconds();
    assert!(
        (599..=601).contains(&minutes),
        "about 10 minutes, got {minutes}s"
    );

    let until = db::rfc3339(Utc::now() + chrono::Duration::minutes(90));
    let (status, card) = send(
        &r.app,
        "POST",
        &format!("{BREAK_URL}/show"),
        Some(json!({"until": until})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(card["endsAt"], until);
    assert_eq!(snapshot_break(&r.bus)["payload"]["endsAt"], until);

    for bad in [
        json!({}),
        json!({"minutes": 5, "until": until}),
        json!({"minutes": 0}),
        json!({"until": "not a time"}),
        json!({"until": db::rfc3339(Utc::now() - chrono::Duration::minutes(1))}),
    ] {
        let (status, body) = send(&r.app, "POST", &format!("{BREAK_URL}/show"), Some(bad)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], "overlays.invalid");
    }

    let (status, card) = send(&r.app, "POST", &format!("{BREAK_URL}/hide"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(card["shown"], false);
    assert_eq!(card["endsAt"], Value::Null);
}

#[tokio::test]
async fn a_restart_keeps_the_countdown() {
    let r = rig().await;
    let (_, shown) = send(
        &r.app,
        "POST",
        &format!("{BREAK_URL}/show"),
        Some(json!({"minutes": 5})),
    )
    .await;
    let again = restart(r.pool.clone()).await;
    let card = snapshot_break(&again.bus);
    assert_eq!(card["payload"]["shown"], true);
    assert_eq!(
        card["payload"]["endsAt"], shown["endsAt"],
        "the same end, not counted again"
    );
}

#[test]
fn the_event_type_matches_what_the_bus_sends() {
    let bus = EventBus::default();
    bus.retain(BREAK, 1);
    bus.publish(Event {
        id: "b".into(),
        event_type: BREAK,
        version: BREAK_VERSION,
        occurred_at: "2026-11-22T12:00:00.000Z".into(),
        source: Source::System,
        payload: serde_json::to_value(BreakPayload::from(&BreakCard::default())).unwrap(),
    });
    let wire = snapshot_break(&bus);
    let typed: BreakEvent = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), wire);
}
