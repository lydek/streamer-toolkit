use super::*;
use axum::{body::Body, http::Request};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use toolkit_core::{
    commands::Declared,
    events::{ChatBadge, ChatMessage},
};
use tower::ServiceExt;

struct Rig {
    dir: PathBuf,
    pool: SqlitePool,
    bus: Arc<EventBus>,
    modules: Arc<Modules>,
    commands: Arc<CommandTable>,
    session: Arc<Mutex<Option<String>>>,
    service: Arc<Counters>,
    app: Router,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const CHECKIN: Declared = Declared {
    key: "checkin",
    module: "checkin",
    default_name: "打卡",
};

async fn rig(name: &str) -> Rig {
    let dir = std::env::temp_dir().join(format!("st-counter-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let pool = db::tests::pool().await;
    let session = Arc::new(Mutex::new(Some("s1".to_owned())));
    restart(dir, pool, session).await
}

/// The service again on the same data, as after restarting the app.
async fn restart(dir: PathBuf, pool: SqlitePool, session: Arc<Mutex<Option<String>>>) -> Rig {
    let bus = Arc::new(EventBus::default());
    let modules = Arc::new(Modules::load(dir.clone()));
    let commands = Arc::new(CommandTable::load(dir.clone(), vec![CHECKIN]));
    let now = session.clone();
    let service = start(
        pool.clone(),
        bus.clone(),
        modules.clone(),
        commands.clone(),
        move || {
            let s = now.lock().unwrap().clone();
            async move { s }
        },
    );
    // Let the counters load.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let app = routes(service.clone());
    Rig {
        dir,
        pool,
        bus,
        modules,
        commands,
        session,
        service,
        app,
    }
}

async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if body.is_some() {
        req = req.header("content-type", "application/json");
    }
    let res = app
        .clone()
        .oneshot(
            req.body(body.map_or(Body::empty(), |b| Body::from(b.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn create(r: &Rig, name: &str, command: &str, per_stream: bool) -> String {
    let (status, body) = call(
        &r.app,
        "POST",
        "/api/admin/counters",
        Some(json!({ "name": name, "command": command, "allowed": "everyone", "per_stream": per_stream })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["id"].as_str().unwrap().to_owned()
}

async fn value(r: &Rig, id: &str) -> i64 {
    let (_, all) = call(&r.app, "GET", "/api/admin/counters", None).await;
    all.as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .unwrap()["value"]
        .as_i64()
        .unwrap()
}

async fn add(r: &Rig, id: &str, delta: i64) -> StatusCode {
    call(
        &r.app,
        "POST",
        &format!("/api/admin/counters/{id}/add"),
        Some(json!({ "delta": delta })),
    )
    .await
    .0
}

fn chat(user: &str, text: &str, badges: &[ChatBadge]) -> ChatMessage {
    ChatMessage {
        id: uuid::Uuid::now_v7().to_string(),
        broadcaster_id: "100".into(),
        user_id: user.into(),
        user_login: "v".into(),
        user_name: "V".into(),
        color: None,
        text: text.into(),
        fragments: Vec::new(),
        badges: badges.to_vec(),
        highlighted: false,
        bits: 0,
        reply: None,
        twitch_badges: Vec::new(),
        shared_from: None,
        sent_at: "2026-11-21T10:00:00Z".into(),
    }
}

/// Lets chat counts already sent finish.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(50)).await;
}

#[tokio::test]
async fn counters_are_created_counted_by_buttons_and_deleted_with_their_command() {
    let r = rig("crud").await;
    let id = create(&r, " 死亡次數 ", "!死亡", true).await;
    let (_, all) = call(&r.app, "GET", "/api/admin/counters", None).await;
    assert_eq!(
        all,
        json!([{ "id": id, "name": "死亡次數", "value": 0, "command": "死亡", "allowed": "everyone", "per_stream": true }])
    );
    assert_eq!(
        r.commands.get(&format!("counter.{id}")).label.as_deref(),
        Some("死亡次數")
    );

    assert_eq!(add(&r, &id, 1).await, StatusCode::NO_CONTENT);
    assert_eq!(add(&r, &id, 1).await, StatusCode::NO_CONTENT);
    assert_eq!(add(&r, &id, -1).await, StatusCode::NO_CONTENT);
    assert_eq!(value(&r, &id).await, 1);
    for _ in 0..3 {
        add(&r, &id, -1).await;
    }
    assert_eq!(value(&r, &id).await, 0, "never below 0");
    assert_eq!(add(&r, &id, 5).await, StatusCode::BAD_REQUEST);
    add(&r, &id, 1).await;
    assert_eq!(
        call(
            &r.app,
            "POST",
            &format!("/api/admin/counters/{id}/reset"),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(value(&r, &id).await, 0);

    let snapshot: Value = serde_json::from_str(&r.bus.subscribe().0).unwrap();
    let event = &snapshot["payload"]["counter.values"][0];
    let typed: CounterValuesEvent = serde_json::from_value(event.clone()).unwrap();
    assert_eq!(
        &serde_json::to_value(typed).unwrap(),
        event,
        "matches its documented type"
    );
    assert_eq!(
        event["payload"]["counters"],
        json!([{ "id": id, "name": "死亡次數", "value": 0 }])
    );

    for bad in [
        json!({ "name": " ", "command": "", "allowed": "everyone", "per_stream": false }),
        json!({ "name": "x", "command": "two words", "allowed": "everyone", "per_stream": false }),
    ] {
        assert_eq!(
            call(&r.app, "POST", "/api/admin/counters", Some(bad))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        call(&r.app, "DELETE", &format!("/api/admin/counters/{id}"), None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        r.commands.saved(&format!("counter.{id}")),
        None,
        "its command is gone"
    );
    assert_eq!(
        call(&r.app, "DELETE", &format!("/api/admin/counters/{id}"), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_command_another_feature_or_counter_has_is_refused_and_names_it() {
    let r = rig("taken").await;
    create(&r, "死亡次數", "死亡", false).await;
    let input = |command: &str| json!({ "name": "被抓包", "command": command, "allowed": "everyone", "per_stream": false });
    for (command, module, label) in [
        ("!死亡", "counter", "死亡次數"),
        ("打卡", "checkin", "!打卡"),
    ] {
        let (status, body) =
            call(&r.app, "POST", "/api/admin/counters", Some(input(command))).await;
        assert_eq!(status, StatusCode::CONFLICT, "{command}");
        assert_eq!(
            body,
            json!({ "code": "commands.taken", "module": module, "label": label })
        );
    }
    // Case does not matter; a counter keeps its own name when renamed.
    let caught = create(&r, "被抓包", "Caught", false).await;
    let (status, _) = call(&r.app, "POST", "/api/admin/counters", Some(input("CAUGHT"))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, body) = call(
        &r.app,
        "PUT",
        &format!("/api/admin/counters/{caught}"),
        Some(input("caught")),
    )
    .await;
    assert_eq!(
        (status, body["command"].clone()),
        (StatusCode::OK, json!("caught"))
    );
    // No command at all is fine, for several counters.
    create(&r, "A", "", false).await;
    create(&r, "B", "", false).await;
}

#[tokio::test]
async fn chat_commands_count_for_those_allowed_and_never_from_simulations() {
    let r = rig("chat").await;
    let id = create(&r, "死亡次數", "死亡", false).await;
    let input = json!({ "name": "死亡次數", "command": "死亡", "allowed": "moderator", "per_stream": false });
    call(
        &r.app,
        "PUT",
        &format!("/api/admin/counters/{id}"),
        Some(input),
    )
    .await;

    r.service
        .receive_chat(&chat("1", "!死亡", &[]), Source::Twitch);
    r.service
        .receive_chat(&chat("2", "!死亡", &[ChatBadge::Vip]), Source::Twitch);
    r.service
        .receive_chat(&chat("3", "!死亡", &[ChatBadge::Moderator]), Source::Twitch);
    r.service
        .receive_chat(&chat("100", "!死亡 又來", &[]), Source::Twitch);
    r.service
        .receive_chat(&chat("100", "!死亡", &[]), Source::Simulation);
    r.service
        .receive_chat(&chat("100", "死亡", &[]), Source::Twitch);
    settle().await;
    assert_eq!(value(&r, &id).await, 2, "the moderator and the broadcaster");

    r.modules
        .set_enabled(toolkit_core::modules::COUNTER, false)
        .unwrap();
    r.service
        .receive_chat(&chat("100", "!死亡", &[]), Source::Twitch);
    settle().await;
    assert_eq!(value(&r, &id).await, 2, "dropped while disabled");
}

#[tokio::test]
async fn per_stream_counters_reset_once_per_session_and_never_on_a_restart() {
    let r = rig("reset").await;
    // Created during stream s1: not reset at once.
    let per_stream = create(&r, "死亡次數", "死亡", true).await;
    let kept = create(&r, "總計", "總計", false).await;
    for id in [&per_stream, &kept] {
        add(&r, id, 1).await;
    }
    r.service.stream_live().await;
    assert_eq!(
        value(&r, &per_stream).await,
        1,
        "the stream it was created in"
    );

    // The next stream (the minute check finds it after a late start just the same): once.
    *r.session.lock().unwrap() = Some("s2".into());
    r.service.stream_live().await;
    assert_eq!(
        (value(&r, &per_stream).await, value(&r, &kept).await),
        (0, 1)
    );
    add(&r, &per_stream, 1).await;
    r.service.stream_live().await;
    assert_eq!(value(&r, &per_stream).await, 1, "once per session");

    // Restarting during s2 does not reset it again.
    let (dir, pool, session) = (r.dir.clone(), r.pool.clone(), r.session.clone());
    let restarted = restart(dir, pool, session).await;
    restarted.service.stream_live().await;
    assert_eq!(value(&restarted, &per_stream).await, 1);

    // Switching a counter to per-stream mid-stream does not reset it at once either.
    let input =
        json!({ "name": "總計", "command": "總計", "allowed": "everyone", "per_stream": true });
    call(
        &restarted.app,
        "PUT",
        &format!("/api/admin/counters/{kept}"),
        Some(input),
    )
    .await;
    restarted.service.stream_live().await;
    assert_eq!(value(&restarted, &kept).await, 1);
    *restarted.session.lock().unwrap() = Some("s3".into());
    restarted.service.stream_live().await;
    assert_eq!(
        (
            value(&restarted, &per_stream).await,
            value(&restarted, &kept).await
        ),
        (0, 0)
    );
    // The first rig shares the data folder; only the restarted one cleans it up.
    std::mem::forget(r);
}

#[tokio::test]
async fn a_counter_made_before_any_stream_resets_at_the_first() {
    let r = rig("first").await;
    *r.session.lock().unwrap() = None;
    let id = create(&r, "死亡次數", "", true).await;
    add(&r, &id, 1).await;
    r.service.stream_live().await;
    assert_eq!(
        value(&r, &id).await,
        1,
        "no stream yet: nothing to reset for"
    );
    *r.session.lock().unwrap() = Some("s1".into());
    r.service.stream_live().await;
    assert_eq!(value(&r, &id).await, 0);
}
