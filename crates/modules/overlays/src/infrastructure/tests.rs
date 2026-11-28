use super::*;
use axum::{body::Body, http::Request};
use serde_json::{Value, json};
use std::time::Duration;
use toolkit_core::time::{SignedDuration, Timestamp};
use tower::ServiceExt;

struct Rig {
    dir: std::path::PathBuf,
    pool: SqlitePool,
    bus: Arc<EventBus>,
    service: Arc<Overlays>,
    app: Router,
}

async fn rig() -> Rig {
    restart(db::tests::pool().await).await
}

/// A data folder of its own for each service started (removed with the process's temp files).
fn data_dir() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("st-overlays-{}-{n}", std::process::id()))
}

/// The service again on the same data, as after restarting the app.
async fn restart(pool: SqlitePool) -> Rig {
    restart_in(pool, data_dir()).await
}

async fn restart_in(pool: SqlitePool, dir: std::path::PathBuf) -> Rig {
    let bus = Arc::new(EventBus::default());
    let presenter = Presenter::start(Duration::from_millis(10), |_| true);
    let service = start(
        pool.clone(),
        bus.clone(),
        Deps {
            data_dir: dir.clone(),
            presenter,
            modules: Arc::new(Modules::load(dir.clone())),
            master_volume: Box::new(|| 50),
        },
    );
    // Let it announce the stored card.
    tokio::time::sleep(Duration::from_millis(50)).await;
    Rig {
        dir,
        pool,
        bus,
        app: routes(service.clone()),
        service,
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
    let before = Timestamp::now();
    let (status, card) = send(
        &r.app,
        "POST",
        &format!("{BREAK_URL}/show"),
        Some(json!({"minutes": 10})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(card["shown"], true);
    let ends = (card["endsAt"].as_str().unwrap())
        .parse::<Timestamp>()
        .unwrap();
    let minutes = (ends - before).as_secs();
    assert!(
        (599..=601).contains(&minutes),
        "about 10 minutes, got {minutes}s"
    );

    let until = db::rfc3339(Timestamp::now() + SignedDuration::from_mins(90));
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
        json!({"until": db::rfc3339(Timestamp::now() - SignedDuration::from_mins(1))}),
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

/// The latest `screen.warnings` payload in a new snapshot.
fn snapshot_warnings(bus: &EventBus) -> Value {
    let (snapshot, _) = bus.subscribe();
    let v: Value = serde_json::from_str(&snapshot).unwrap();
    v["payload"]["screen.warnings"][0]["payload"].clone()
}

fn seen(chat: bool, twitch: bool, mic: Option<bool>) -> crate::Conditions {
    crate::Conditions {
        chat_overlay_open: chat,
        twitch_connected: twitch,
        inputs: Some(
            mic.map(|muted| toolkit_core::streaming::Input {
                name: "Mic".into(),
                muted,
                ..Default::default()
            })
            .into_iter()
            .collect(),
        ),
    }
}

const WARNINGS_URL: &str = "/api/admin/screen-tools/warnings";

#[tokio::test]
async fn warnings_show_and_clear_with_what_the_host_sees() {
    let r = rig().await;
    let w = snapshot_warnings(&r.bus);
    assert_eq!(w["chatOverlay"], true);
    assert_eq!(w["twitch"], false);
    assert_eq!(w["micSeconds"], 30);
    assert_eq!(w["micInput"], Value::Null);

    r.service
        .see(seen(false, false, Some(true)), Timestamp::now());
    let w = snapshot_warnings(&r.bus);
    assert_eq!(w["chatOverlayDown"], true);
    assert_eq!(w["twitchDown"], true);
    assert_eq!(
        w["micMutedSince"],
        Value::Null,
        "no input picked: not checked"
    );

    let settings = json!({"chat_overlay": true, "twitch": true, "mic": true, "mic_seconds": 10, "mic_input": "Mic"});
    let (status, w) = send(&r.app, "PUT", WARNINGS_URL, Some(settings)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(w["micInput"], "Mic");
    assert!(
        w["micMutedSince"].is_string(),
        "the muted mic starts its wait"
    );
    assert_eq!(snapshot_warnings(&r.bus)["micSeconds"], 10);

    r.service
        .see(seen(true, true, Some(false)), Timestamp::now());
    let w = snapshot_warnings(&r.bus);
    assert_eq!(w["chatOverlayDown"], false);
    assert_eq!(w["twitchDown"], false);
    assert_eq!(w["micMutedSince"], Value::Null);

    r.service.see(seen(true, true, None), Timestamp::now());
    assert_eq!(snapshot_warnings(&r.bus)["micMissing"], true);
    let (_, w) = send(&r.app, "GET", WARNINGS_URL, None).await;
    assert_eq!(w["micMissing"], true);

    let bad = json!({"chat_overlay": true, "twitch": true, "mic": true, "mic_seconds": 0, "mic_input": null});
    let (status, body) = send(&r.app, "PUT", WARNINGS_URL, Some(bad)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "overlays.invalid");
}

#[tokio::test]
async fn warning_settings_survive_a_restart_and_unchanged_sights_announce_nothing() {
    let r = rig().await;
    let settings = json!({"chat_overlay": false, "twitch": true, "mic": false, "mic_seconds": 45, "mic_input": "Mic"});
    send(&r.app, "PUT", WARNINGS_URL, Some(settings)).await;
    let again = restart(r.pool.clone()).await;
    let w = snapshot_warnings(&again.bus);
    assert_eq!(w["chatOverlay"], false);
    assert_eq!(w["micSeconds"], 45);
    assert_eq!(w["micInput"], "Mic");

    let (_, mut events) = again.bus.subscribe();
    again
        .service
        .see(seen(true, true, Some(false)), Timestamp::now());
    assert!(events.try_recv().is_ok());
    again
        .service
        .see(seen(true, true, Some(false)), Timestamp::now());
    assert!(
        events.try_recv().is_err(),
        "the same sight again is not announced"
    );
}

#[test]
fn the_warnings_event_type_matches_what_the_bus_sends() {
    let bus = EventBus::default();
    bus.retain(warnings::WARNINGS, 1);
    let w = crate::domain::warnings::Warnings {
        mic_muted_since: Some(Timestamp::now()),
        ..Default::default()
    };
    bus.publish(Event {
        id: "w".into(),
        event_type: warnings::WARNINGS,
        version: warnings::WARNINGS_VERSION,
        occurred_at: "2026-11-22T12:00:00.000Z".into(),
        source: Source::System,
        payload: serde_json::to_value(warnings::WarningsPayload::new(&Default::default(), &w))
            .unwrap(),
    });
    let (snapshot, _) = bus.subscribe();
    let v: Value = serde_json::from_str(&snapshot).unwrap();
    let wire = v["payload"]["screen.warnings"][0].clone();
    let typed: WarningsEvent = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), wire);
}

#[tokio::test]
async fn the_appearance_is_saved_announced_and_checked() {
    let r = rig().await;
    let appearance = |bus: &EventBus| {
        let (snapshot, _) = bus.subscribe();
        let v: Value = serde_json::from_str(&snapshot).unwrap();
        v["payload"]["screen.appearance"][0].clone()
    };
    assert_eq!(
        appearance(&r.bus)["payload"],
        json!({"theme": "dark", "textSize": 100})
    );

    let url = "/api/admin/screen-tools/appearance";
    let (status, body) = send(
        &r.app,
        "PUT",
        url,
        Some(json!({"theme": "cream", "textSize": 130})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"theme": "cream", "textSize": 130}));
    let wire = appearance(&r.bus);
    assert_eq!(wire["payload"], body);
    let typed: AppearanceEvent = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), wire);

    for bad in [
        json!({"theme": "cream", "textSize": 105}),
        json!({"theme": "light", "textSize": 100}),
    ] {
        let (status, body) = send(&r.app, "PUT", url, Some(bad)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], "overlays.invalid");
    }

    let again = restart(r.pool.clone()).await;
    assert_eq!(appearance(&again.bus)["payload"]["theme"], "cream");
    let (_, body) = send(&again.app, "GET", url, None).await;
    assert_eq!(body["textSize"], 130);
}

async fn upload(app: &Router, name: &str, bytes: &[u8]) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/admin/screen-tools/media?name={name}"))
        .header("content-type", "application/octet-stream")
        .body(Body::from(bytes.to_vec()))
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

const MEDIA_URL: &str = "/api/admin/screen-tools/media";
const GIF: &[u8] = b"GIF89a  ";
const MP4: &[u8] = b"   ftypmp42    ";

#[tokio::test]
async fn media_import_checks_the_type_and_keeps_the_file() {
    let r = rig().await;
    let (status, gif) = upload(&r.app, "cat%20dance.gif", GIF).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(gif["title"], "cat dance");
    assert_eq!(gif["format"], "gif");
    assert_eq!(gif["duration_ms"], 5000, "a GIF starts at 5 seconds");
    assert_eq!(gif["missing"], false);
    let id = gif["id"].as_str().unwrap();
    assert_eq!(
        std::fs::read(r.dir.join("media").join(format!("{id}.gif"))).unwrap(),
        GIF
    );

    let (status, video) = upload(&r.app, "scare.mp4", MP4).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        video["duration_ms"],
        Value::Null,
        "a video waits for its read length"
    );

    for (name, bytes) in [("fake.gif", MP4), ("fake.mp4", GIF), ("clip.mov", MP4)] {
        let (status, body) = upload(&r.app, name, bytes).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}");
        assert_eq!(body["code"], "media.unsupported_type");
    }
    let big = vec![b'G'; crate::domain::media::MAX_BYTES + 1];
    let (status, body) = upload(&r.app, "big.gif", &big).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["code"], "media.too_large");

    let (_, list) = send(&r.app, "GET", MEDIA_URL, None).await;
    assert_eq!(
        list.as_array().unwrap().len(),
        2,
        "refused files leave nothing behind"
    );
    assert_eq!(std::fs::read_dir(r.dir.join("media")).unwrap().count(), 2);
}

#[tokio::test]
async fn media_lengths_are_edited_and_files_served_and_deleted() {
    let r = rig().await;
    let (_, video) = upload(&r.app, "scare.mp4", MP4).await;
    let id = video["id"].as_str().unwrap().to_owned();
    let url = format!("{MEDIA_URL}/{id}");
    let (status, video) = send(
        &r.app,
        "PUT",
        &url,
        Some(json!({"title": "驚嚇", "duration_ms": 3200})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(video["title"], "驚嚇");
    assert_eq!(video["duration_ms"], 3200);
    for bad in [
        json!({"title": " ", "duration_ms": null}),
        json!({"title": "x", "duration_ms": 0}),
    ] {
        let (status, body) = send(&r.app, "PUT", &url, Some(bad)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], "media.invalid");
    }

    let files = media_file_routes(r.service.clone());
    let res = files
        .clone()
        .oneshot(
            Request::get(format!("/media/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[axum::http::header::CONTENT_TYPE], "video/mp4");
    assert_eq!(
        res.headers()[axum::http::header::X_CONTENT_TYPE_OPTIONS],
        "nosniff"
    );

    let (status, _) = send(&r.app, "DELETE", &url, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(
        std::fs::read_dir(r.dir.join("media"))
            .unwrap()
            .next()
            .is_none()
    );
    let res = files
        .oneshot(
            Request::get(format!("/media/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let (status, body) = send(&r.app, "DELETE", &url, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "media.not_found");
}

#[tokio::test]
async fn a_media_file_gone_from_the_folder_is_marked_missing() {
    let r = rig().await;
    let (_, gif) = upload(&r.app, "a.gif", GIF).await;
    std::fs::remove_dir_all(r.dir.join("media")).unwrap();
    let again = restart_in(r.pool.clone(), r.dir.clone()).await;
    let (_, list) = send(&again.app, "GET", MEDIA_URL, None).await;
    assert_eq!(list[0]["id"], gif["id"]);
    assert_eq!(list[0]["missing"], true);
}

#[tokio::test]
async fn an_entry_whose_id_is_not_a_uuid_never_reaches_outside_the_media_folder() {
    let r = rig().await;
    upload(&r.app, "a.gif", GIF).await;
    // As an edited database restored from a backup could hold.
    sqlx::query("UPDATE overlays_media SET id = '../escape'")
        .execute(&r.pool)
        .await
        .unwrap();
    let outside = r.dir.join("escape.gif");
    std::fs::write(&outside, GIF).unwrap();
    let (_, list) = send(&r.app, "GET", MEDIA_URL, None).await;
    assert_eq!(list[0]["missing"], true);
    assert_eq!(r.service.media_file("../escape").await, None);
    r.service.delete_media("../escape").await.unwrap();
    assert!(outside.is_file(), "the file outside media/ stays");
    let (_, list) = send(&r.app, "GET", MEDIA_URL, None).await;
    assert_eq!(list, json!([]));
}

fn redemption(
    id: &str,
    reward_id: &str,
    change: toolkit_core::events::RedemptionChange,
) -> toolkit_core::events::ChannelPointsRedemption {
    toolkit_core::events::ChannelPointsRedemption {
        id: id.into(),
        change,
        user_id: "u1".into(),
        user_login: "orange".into(),
        user_name: "橘子".into(),
        reward_id: reward_id.into(),
        reward_title: "貓咪".into(),
        reward_cost: 100,
        user_input: String::new(),
        status: toolkit_core::events::RedemptionStatus::Unfulfilled,
        redeemed_at: "2026-11-22T12:00:00Z".into(),
    }
}

/// Every `screen.media` event published so far, as (source, payload).
fn media_events(events: &mut tokio::sync::broadcast::Receiver<Arc<str>>) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    while let Ok(text) = events.try_recv() {
        let v: Value = serde_json::from_str(&text).unwrap();
        if v["type"] == "screen.media" {
            out.push((
                v["source"].as_str().unwrap().to_owned(),
                v["payload"].clone(),
            ));
        }
    }
    out
}

#[tokio::test]
async fn a_redemption_queues_its_reward_media_once_and_a_test_records_nothing() {
    use toolkit_core::events::RedemptionChange::{Added, Updated};
    let r = rig().await;
    let (_, gif) = upload(&r.app, "cat.gif", GIF).await;
    let id = gif["id"].as_str().unwrap().to_owned();
    send(
        &r.app,
        "PUT",
        &format!("{MEDIA_URL}/{id}"),
        Some(json!({"title": "貓咪", "duration_ms": 20})),
    )
    .await;
    let rule = json!({"media_id": id, "position": "bottom_right", "volume": null});
    let (status, body) = send(
        &r.app,
        "PUT",
        "/api/admin/screen-tools/media-rules/rw1",
        Some(rule),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["position"], "bottom_right");
    let (_, rules) = send(&r.app, "GET", "/api/admin/screen-tools/media-rules", None).await;
    assert_eq!(rules[0]["reward_id"], "rw1");

    let (_, mut events) = r.bus.subscribe();
    r.service
        .receive_redemption(&redemption("x1", "rw1", Added));
    r.service
        .receive_redemption(&redemption("x1", "rw1", Updated));
    r.service
        .receive_redemption(&redemption("x2", "other", Added));
    tokio::time::sleep(Duration::from_millis(150)).await;
    let shown = media_events(&mut events);
    assert_eq!(shown.len(), 1, "one redemption, one show");
    assert_eq!(shown[0].0, "twitch");
    assert_eq!(
        shown[0].1,
        json!({"url": format!("/media/{id}"), "format": "gif", "position": "bottom_right", "volume": 50, "durationMs": 20})
    );
    let recent = r.service.media_recent();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].item.key.as_deref(), Some("x1"));
    assert_eq!(
        recent[0].item.outcome,
        Some(toolkit_core::presentation::Outcome::Played)
    );

    let (status, _) = send(
        &r.app,
        "POST",
        "/api/admin/screen-tools/media-rules/rw1/test",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let shown = media_events(&mut events);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].0, "simulation");
    assert_eq!(r.service.media_recent().len(), 1, "a test is not recorded");
    let (status, body) = send(
        &r.app,
        "POST",
        "/api/admin/screen-tools/media-rules/none/test",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "media.not_found");
}

#[tokio::test]
async fn a_redemption_never_plays_a_file_outside_the_media_folder() {
    use toolkit_core::{events::RedemptionChange::Added, presentation::Outcome};
    let r = rig().await;
    let (_, gif) = upload(&r.app, "cat.gif", GIF).await;
    let id = gif["id"].as_str().unwrap().to_owned();
    let rule = json!({"media_id": id, "position": "full", "volume": 80});
    send(
        &r.app,
        "PUT",
        "/api/admin/screen-tools/media-rules/rw1",
        Some(rule),
    )
    .await;
    // As an edited database restored from a backup could hold, with a file waiting outside media/.
    for sql in [
        "UPDATE overlays_media SET id = '../escape'",
        "UPDATE overlays_media_rules SET media_id = '../escape'",
    ] {
        sqlx::query(sql).execute(&r.pool).await.unwrap();
    }
    std::fs::write(r.dir.join("escape.gif"), GIF).unwrap();
    let again = restart_in(r.pool.clone(), r.dir.clone()).await;

    let (_, mut events) = again.bus.subscribe();
    again
        .service
        .receive_redemption(&redemption("x1", "rw1", Added));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(media_events(&mut events).is_empty());
    let recent = again.service.media_recent();
    assert_eq!(recent[0].item.outcome, Some(Outcome::Declined));
    assert!(recent[0].file_missing);
}

#[tokio::test]
async fn a_missing_file_is_declined_at_once_and_says_why() {
    use toolkit_core::{events::RedemptionChange::Added, presentation::Outcome};
    let r = rig().await;
    let (_, gif) = upload(&r.app, "cat.gif", GIF).await;
    let id = gif["id"].as_str().unwrap().to_owned();
    let rule = json!({"media_id": id, "position": "full", "volume": 80});
    send(
        &r.app,
        "PUT",
        "/api/admin/screen-tools/media-rules/rw1",
        Some(rule),
    )
    .await;
    std::fs::remove_file(r.dir.join("media").join(format!("{id}.gif"))).unwrap();

    let (_, mut events) = r.bus.subscribe();
    r.service
        .receive_redemption(&redemption("x1", "rw1", Added));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(media_events(&mut events).is_empty());
    let recent = r.service.media_recent();
    assert_eq!(recent[0].item.outcome, Some(Outcome::Declined));
    assert!(recent[0].file_missing);

    // Deleting the item takes its rules with it.
    send(&r.app, "DELETE", &format!("{MEDIA_URL}/{id}"), None).await;
    let (_, rules) = send(&r.app, "GET", "/api/admin/screen-tools/media-rules", None).await;
    assert_eq!(rules, json!([]));
    let (status, _) = send(
        &r.app,
        "DELETE",
        "/api/admin/screen-tools/media-rules/rw1",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_media_event_type_matches_what_the_bus_sends() {
    let r = rig().await;
    let (_, gif) = upload(&r.app, "cat.gif", GIF).await;
    let rule = json!({"media_id": gif["id"], "position": "center", "volume": 30});
    send(
        &r.app,
        "PUT",
        "/api/admin/screen-tools/media-rules/rw1",
        Some(rule),
    )
    .await;
    let (_, mut events) = r.bus.subscribe();
    r.service.test_media("rw1").unwrap();
    let wire: Value = serde_json::from_str(&events.try_recv().unwrap()).unwrap();
    let typed: MediaPlayEvent = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), wire);
}
