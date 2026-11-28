use super::*;
use toolkit_core::time::{SignedDuration, Timestamp};

async fn send(
    app: &axum::Router,
    method: &str,
    body: Option<serde_json::Value>,
) -> (u16, serde_json::Value) {
    let req = Request::builder()
        .method(method)
        .uri("/api/admin/checkin/settings")
        .header("content-type", "application/json")
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status().as_u16();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

#[tokio::test]
async fn settings_are_validated_saved_and_applied() {
    let dir = data_dir("http-settings");
    let modules = Arc::new(Modules::load(dir.clone()));
    let pool = pool().await;
    let service = start_quiet(
        pool.clone(),
        Arc::new(EventBus::default()),
        modules,
        dir.clone(),
    );
    let app = http::routes(pool, Arc::new(service));

    let (status, body) = send(&app, "GET", None).await;
    assert_eq!(status, 200);
    assert_eq!(
        body,
        serde_json::json!({ "command": "打卡", "allowed": "everyone", "time_zone": "Asia/Taipei", "reset_hour": 0, "card_slots": 10, "reward_id": null })
    );

    let next = serde_json::json!({ "command": " !Sign ", "allowed": "subscriber", "time_zone": "Europe/Berlin", "reset_hour": 6, "card_slots": 8 });
    let (status, body) = send(&app, "PUT", Some(next)).await;
    assert_eq!(status, 200);
    assert_eq!(body["command"], "Sign", "stored without its !");
    assert_eq!(body["allowed"], "subscriber");
    let all = toolkit_core::settings::load(&dir);
    let (saved, command) = (all.checkin, &all.chat_commands["checkin"]);
    assert_eq!(
        (
            command.name.as_str(),
            command.allowed,
            saved.time_zone.as_str(),
            saved.reset_hour,
            saved.card_slots
        ),
        (
            "Sign",
            toolkit_core::commands::Allowed::Subscriber,
            "Europe/Berlin",
            6,
            8
        ),
        "the command in the command table"
    );

    for bad in [
        serde_json::json!({ "command": "two words", "allowed": "everyone", "time_zone": "UTC", "reset_hour": 0, "card_slots": 10 }),
        serde_json::json!({ "command": "x", "allowed": "everyone", "time_zone": "Nowhere/City", "reset_hour": 0, "card_slots": 10 }),
        serde_json::json!({ "command": "x", "allowed": "everyone", "time_zone": "UTC", "reset_hour": 24, "card_slots": 10 }),
        serde_json::json!({ "command": "x", "allowed": "everyone", "time_zone": "UTC", "reset_hour": 0, "card_slots": 0 }),
    ] {
        let (status, body) = send(&app, "PUT", Some(bad.clone())).await;
        assert_eq!(
            (status, &body["code"]),
            (400, &serde_json::json!("checkin.invalid_settings")),
            "{bad}"
        );
    }
    let (_, body) = send(&app, "GET", None).await;
    assert_eq!(body["command"], "Sign", "a refused update changes nothing");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_command_name_another_feature_has_is_refused_and_names_it() {
    use toolkit_core::commands::{CommandTable, Declared};
    let dir = data_dir("http-taken");
    let declared = [
        crate::COMMANDS,
        &[Declared {
            key: "counter.deaths",
            module: "counter",
            default_name: "Deaths",
        }],
    ]
    .concat();
    let service = crate::start(
        pool().await,
        Arc::new(EventBus::default()),
        Arc::new(Modules::load(dir.clone())),
        dir.clone(),
        Arc::new(NoProfiles),
        sounds(&dir),
        Timing::default(),
        presenter(),
        Arc::new(CommandTable::load(dir.clone(), declared)),
    );
    let app = http::routes(pool().await, Arc::new(service));
    for name in ["Deaths", "!DEATHS"] {
        let body = serde_json::json!({ "command": name, "allowed": "everyone", "time_zone": "UTC", "reset_hour": 0, "card_slots": 10 });
        let (status, body) = send(&app, "PUT", Some(body)).await;
        assert_eq!(status, 409, "{name}");
        assert_eq!(
            body,
            serde_json::json!({ "code": "commands.taken", "module": "counter", "label": "!Deaths" })
        );
    }
    let (_, body) = send(&app, "GET", None).await;
    assert_eq!(body["command"], "打卡", "nothing changed");
    let _ = std::fs::remove_dir_all(dir);
}

/// A Host-like setup: the module's routes and its bus, with check-ins recorded straight into the database.
struct Api {
    pool: SqlitePool,
    app: axum::Router,
    bus: Arc<EventBus>,
    service: Arc<CheckinService>,
    dir: PathBuf,
}

async fn api(name: &str) -> Api {
    let pool = pool().await;
    let dir = data_dir(name);
    let bus = Arc::new(EventBus::default());
    let service = Arc::new(start_quiet(
        pool.clone(),
        bus.clone(),
        Arc::new(Modules::load(dir.clone())),
        dir.clone(),
    ));
    let app = http::routes(pool.clone(), service.clone());
    Api {
        pool,
        app,
        bus,
        service,
        dir,
    }
}

async fn seed(pool: &SqlitePool, user: &str, name: &str, at: &str) {
    use crate::{
        application::CheckinRepository,
        domain::{BusinessDay, CheckIn, Trigger},
    };
    let rule = BusinessDay::new("Asia/Taipei", 0).unwrap();
    let at = at.parse::<Timestamp>().unwrap();
    let viewer = Viewer {
        twitch_id: TwitchUserId(user.into()),
        login: format!("login{user}"),
        name: name.into(),
    };
    db::SqliteCheckins::new(pool.clone())
        .check_in(&viewer, &CheckIn::at(&rule, at, Trigger::Chat, at))
        .await
        .unwrap();
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> (u16, serde_json::Value) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status().as_u16();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

fn names(page: &serde_json::Value) -> Vec<&str> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["user_name"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn viewers_are_listed_with_totals_cards_and_search() {
    let a = api("viewers").await;
    // Ann checks in on three days, Bob once (latest), <b>Cat</b> once (earliest).
    for at in [
        "2026-11-10T01:00:00Z",
        "2026-11-11T01:00:00Z",
        "2026-11-12T01:00:00Z",
    ] {
        seed(&a.pool, "1", "Ann", at).await;
    }
    seed(&a.pool, "2", "Bob", "2026-11-14T01:00:00Z").await;
    seed(&a.pool, "3", "<b>Cat</b>", "2026-11-01T01:00:00Z").await;

    let (status, page) = call(&a.app, "GET", "/api/admin/checkin/viewers", None).await;
    assert_eq!(status, 200);
    assert_eq!(page["total"], 3);
    assert_eq!(
        names(&page),
        ["Bob", "Ann", "<b>Cat</b>"],
        "latest check-in first; text raw"
    );
    let ann = &page["items"][1];
    assert_eq!(
        (&ann["total"], &ann["stamps"], &ann["card_slots"]),
        (&3.into(), &3.into(), &10.into())
    );
    assert_eq!(ann["last_check_in"], "2026-11-12");
    assert_eq!(ann["user_login"], "login1");

    let (_, page) = call(&a.app, "GET", "/api/admin/checkin/viewers?search=AN", None).await;
    assert_eq!(names(&page), ["Ann"]);
    let (_, page) = call(
        &a.app,
        "GET",
        "/api/admin/checkin/viewers?search=login3",
        None,
    )
    .await;
    assert_eq!(names(&page), ["<b>Cat</b>"]);
    let (_, page) = call(&a.app, "GET", "/api/admin/checkin/viewers?search=%25", None).await;
    assert_eq!(page["total"], 0, "% is literal");
    let (_, page) = call(
        &a.app,
        "GET",
        "/api/admin/checkin/viewers?limit=1&offset=1",
        None,
    )
    .await;
    assert_eq!((names(&page), &page["total"]), (vec!["Ann"], &3.into()));
    for bad in ["?limit=0", "?limit=101", "?offset=-1"] {
        let (status, body) = call(
            &a.app,
            "GET",
            &format!("/api/admin/checkin/viewers{bad}"),
            None,
        )
        .await;
        assert_eq!(
            (status, &body["code"]),
            (400, &"checkin.invalid_query".into()),
            "{bad}"
        );
    }

    // A new card size recomputes every card at once.
    let settings = serde_json::json!({ "command": "打卡", "allowed": "everyone", "time_zone": "Asia/Taipei", "reset_hour": 0, "card_slots": 2, "reward_id": null });
    assert_eq!(
        call(&a.app, "PUT", "/api/admin/checkin/settings", Some(settings))
            .await
            .0,
        200
    );
    let (_, page) = call(&a.app, "GET", "/api/admin/checkin/viewers?search=Ann", None).await;
    assert_eq!(
        (&page["items"][0]["stamps"], &page["items"][0]["card_slots"]),
        (&1.into(), &2.into())
    );
    let _ = std::fs::remove_dir_all(&a.dir);
}

#[tokio::test]
async fn manual_check_ins_follow_the_one_per_day_rule() {
    let a = api("manual").await;
    let (_, mut rx) = a.bus.subscribe();
    seed(&a.pool, "1", "Ann", "2026-11-01T01:00:00Z").await;
    let (_, page) = call(&a.app, "GET", "/api/admin/checkin/viewers", None).await;
    let id = page["items"][0]["id"].as_str().unwrap().to_owned();
    let uri = format!("/api/admin/checkin/viewers/{id}/check-ins");

    let today = serde_json::json!({});
    assert_eq!(
        call(&a.app, "POST", &uri, Some(today.clone())).await,
        (200, serde_json::json!({ "result": "checked_in" }))
    );
    assert_eq!(
        call(&a.app, "POST", &uri, Some(today)).await.1["result"],
        "already_checked_in"
    );
    // Back-fill a past day, then the same day again.
    let past = serde_json::json!({ "date": "2026-11-05" });
    assert_eq!(
        call(&a.app, "POST", &uri, Some(past.clone())).await.1["result"],
        "checked_in"
    );
    assert_eq!(
        call(&a.app, "POST", &uri, Some(past)).await.1["result"],
        "already_checked_in"
    );
    assert_eq!(count(&a.pool).await, 3);
    let kinds: Vec<String> =
        sqlx::query_scalar("SELECT trigger_kind FROM checkin_records ORDER BY business_day")
            .fetch_all(&a.pool)
            .await
            .unwrap();
    assert_eq!(kinds, ["chat", "manual", "manual"]);

    for (date, code) in [
        ("2026-13-01", "checkin.invalid_date"),
        ("2999-01-01", "checkin.invalid_date"),
    ] {
        let (status, body) = call(
            &a.app,
            "POST",
            &uri,
            Some(serde_json::json!({ "date": date })),
        )
        .await;
        assert_eq!((status, &body["code"]), (400, &code.into()), "{date}");
    }
    let (status, body) = call(
        &a.app,
        "POST",
        "/api/admin/checkin/viewers/nobody/check-ins",
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(
        (status, &body["code"]),
        (404, &"checkin.viewer_not_found".into())
    );

    // Each recorded one is announced with the viewer ID only; repeats are not.
    let mut changes = Vec::new();
    while let Ok(text) = rx.try_recv() {
        let wire: serde_json::Value = serde_json::from_str(&text).unwrap();
        if wire["type"] == CHANGED {
            changes.push(wire);
        }
    }
    assert_eq!(changes.len(), 2, "repeats are not announced");
    for wire in changes {
        assert_eq!(wire["payload"], serde_json::json!({ "viewer_id": id }));
        let typed: CheckinChangedEvent = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(typed).unwrap(),
            wire,
            "the schema type matches the bus"
        );
    }
    let _ = std::fs::remove_dir_all(&a.dir);
}

#[tokio::test]
async fn today_counts_the_current_business_day() {
    let a = api("today").await;
    let now = Timestamp::now();
    let today = a.service.config().rules().day.day_of(now);
    seed(&a.pool, "1", "Ann", &now.to_string()).await;
    seed(
        &a.pool,
        "2",
        "Bob",
        &(now + SignedDuration::from_millis(1)).to_string(),
    )
    .await;
    seed(&a.pool, "3", "Old", "2026-01-01T01:00:00Z").await;
    let (status, body) = call(&a.app, "GET", "/api/admin/checkin/today", None).await;
    assert_eq!(status, 200);
    assert_eq!(body["business_day"], today.to_string());
    assert_eq!(body["count"], 2);
    let recent: Vec<&str> = body["recent"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["user_name"].as_str().unwrap())
        .collect();
    assert_eq!(recent, ["Bob", "Ann"]);
    let _ = std::fs::remove_dir_all(&a.dir);
}

#[tokio::test]
async fn today_counts_full_cards_and_each_recent_viewers_card_from_the_lifetime_count() {
    let a = api("today_full").await;
    let settings = |slots: u32| serde_json::json!({ "command": "打卡", "allowed": "everyone", "time_zone": "Asia/Taipei", "reset_hour": 0, "card_slots": slots, "reward_id": null });
    let now = Timestamp::now();
    // Ann: an older day and today (2 in all); Bob: today only (1).
    seed(&a.pool, "1", "Ann", "2026-01-01T01:00:00Z").await;
    seed(&a.pool, "1", "Ann", &now.to_string()).await;
    seed(
        &a.pool,
        "2",
        "Bob",
        &(now + SignedDuration::from_millis(1)).to_string(),
    )
    .await;
    let today = |slots: u32| {
        let app = a.app.clone();
        let body = settings(slots);
        async move {
            let (status, _) = call(&app, "PUT", "/api/admin/checkin/settings", Some(body)).await;
            assert_eq!(status, 200);
            call(&app, "GET", "/api/admin/checkin/today", None).await.1
        }
    };
    let cards = |body: &serde_json::Value| -> Vec<(String, u64, u64)> {
        body["recent"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["user_name"].as_str().unwrap().to_owned(),
                    r["stamps"].as_u64().unwrap(),
                    r["card_slots"].as_u64().unwrap(),
                )
            })
            .collect()
    };
    // Two slots: Ann's card is full (2/2), Bob's is not (1/2).
    let body = today(2).await;
    assert_eq!(body["full_today"], 1);
    assert_eq!(
        cards(&body),
        [("Bob".to_owned(), 1, 2), ("Ann".to_owned(), 2, 2)]
    );
    // Changing the slots counts again at once: with three, nobody is full.
    let body = today(3).await;
    assert_eq!(body["full_today"], 0);
    assert_eq!(
        cards(&body),
        [("Bob".to_owned(), 1, 3), ("Ann".to_owned(), 2, 3)]
    );
    // With one slot every check-in fills a card.
    assert_eq!(today(1).await["full_today"], 2);
    let _ = std::fs::remove_dir_all(&a.dir);
}

async fn post_raw(app: &axum::Router, uri: &str, body: String) -> (u16, serde_json::Value) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status().as_u16();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

fn viewer_named<'a>(page: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["user_name"] == name)
        .unwrap()
}

#[tokio::test]
async fn import_previews_then_sets_baselines_once() {
    let a = api("import").await;
    seed(&a.pool, "2", "Bob", "2026-11-14T01:00:00Z").await;
    let file = serde_json::json!({
        "1": { "totalCheckIns": 12, "lastName": "<b>Ann</b>", "name": "old", "lastCheckInDate": "2026-10-31", "stamps": 99, "extra": true },
        "2": { "totalCheckIns": 5, "name": "Bobby", "lastCheckInDate": "2026-11-20" },
        "abc": { "totalCheckIns": 1 },
        "3": { "totalCheckIns": 1, "isOwner": true },
        "4": { "totalCheckIns": 1, "excluded": true },
        "5": { "name": "No total" },
        "6": { "totalCheckIns": 1, "lastCheckInDate": "2026/11/01" },
        "7": { "totalCheckIns": 3 },
        // Would overflow baseline + COUNT(*) to REAL in SQLite.
        "8": { "totalCheckIns": i64::MAX }
    })
    .to_string();
    let (status, preview) =
        post_raw(&a.app, "/api/admin/checkin/import/preview", file.clone()).await;
    assert_eq!(status, 200);
    assert_eq!(
        (&preview["added"], &preview["updated"]),
        (&2.into(), &1.into())
    );
    let mut skipped: Vec<(String, String)> = preview["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["key"].as_str().unwrap().into(),
                s["reason"].as_str().unwrap().into(),
            )
        })
        .collect();
    skipped.sort();
    assert_eq!(
        skipped,
        [
            ("3".into(), "owner".into()),
            ("4".into(), "excluded".into()),
            ("5".into(), "invalid_total".into()),
            ("6".into(), "invalid_date".into()),
            ("8".into(), "invalid_total".into()),
            ("abc".into(), "invalid_id".into()),
        ]
    );
    let viewers: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM checkin_viewers")
        .fetch_one(&a.pool)
        .await
        .unwrap();
    assert_eq!(viewers, 1, "a preview writes nothing");

    for round in 0..2 {
        let (status, applied) = post_raw(&a.app, "/api/admin/checkin/import", file.clone()).await;
        assert_eq!(status, 200);
        let expected = if round == 0 { (2, 1) } else { (0, 3) };
        assert_eq!(
            (&applied["added"], &applied["updated"]),
            (&expected.0.into(), &expected.1.into()),
            "round {round}"
        );
        let (_, page) = call(&a.app, "GET", "/api/admin/checkin/viewers", None).await;
        assert_eq!(page["total"], 3);
        let ann = viewer_named(&page, "<b>Ann</b>");
        assert_eq!(
            (&ann["total"], &ann["stamps"]),
            (&12.into(), &2.into()),
            "card from the total, not `stamps`"
        );
        assert_eq!(ann["last_check_in"], "2026-10-31");
        let bob = viewer_named(&page, "Bob");
        assert_eq!(
            bob["total"], 6,
            "baseline 5 plus the toolkit's own check-in; same after a re-import"
        );
        assert_eq!(bob["last_check_in"], "2026-11-20", "the later of the two");
        assert_eq!(
            viewer_named(&page, "7")["last_check_in"],
            serde_json::Value::Null
        );
    }

    // An imported last day never blocks checking in, even on that very day.
    let today = a
        .service
        .config()
        .rules()
        .day
        .day_of(Timestamp::now())
        .to_string();
    let file =
        serde_json::json!({ "1": { "totalCheckIns": 12, "lastCheckInDate": today } }).to_string();
    assert_eq!(
        post_raw(&a.app, "/api/admin/checkin/import", file).await.0,
        200
    );
    let (_, page) = call(&a.app, "GET", "/api/admin/checkin/viewers?search=Ann", None).await;
    let id = page["items"][0]["id"].as_str().unwrap().to_owned();
    let (_, result) = call(
        &a.app,
        "POST",
        &format!("/api/admin/checkin/viewers/{id}/check-ins"),
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(result["result"], "checked_in");
    let (_, page) = call(&a.app, "GET", "/api/admin/checkin/viewers?search=Ann", None).await;
    assert_eq!(page["items"][0]["total"], 13);
    let _ = std::fs::remove_dir_all(&a.dir);
}

#[tokio::test]
async fn import_refuses_what_is_not_an_object_or_too_large() {
    let a = api("import-bad").await;
    for body in ["[1, 2]", "\"text\"", "{not json", ""] {
        for uri in [
            "/api/admin/checkin/import/preview",
            "/api/admin/checkin/import",
        ] {
            let (status, error) = post_raw(&a.app, uri, body.into()).await;
            assert_eq!(
                (status, &error["code"]),
                (400, &"checkin.import_invalid".into()),
                "{uri} {body}"
            );
        }
    }
    let huge = format!(
        "{{\"1\": {{\"totalCheckIns\": 1, \"pad\": \"{}\"}}}}",
        "x".repeat(http::IMPORT_MAX_BYTES)
    );
    let (status, error) = post_raw(&a.app, "/api/admin/checkin/import/preview", huge).await;
    assert_eq!(
        (status, &error["code"]),
        (413, &"checkin.import_too_large".into())
    );
    let _ = std::fs::remove_dir_all(&a.dir);
}

#[tokio::test]
async fn deductions_take_from_what_is_available_never_below_zero() {
    let a = api("deduct").await;
    for at in [
        "2026-11-10T01:00:00Z",
        "2026-11-11T01:00:00Z",
        "2026-11-12T01:00:00Z",
    ] {
        seed(&a.pool, "1", "Ann", at).await;
    }
    let (_, page) = call(&a.app, "GET", "/api/admin/checkin/viewers", None).await;
    let ann = viewer_named(&page, "Ann").clone();
    assert_eq!((&ann["total"], &ann["available"]), (&3.into(), &3.into()));
    let id = ann["id"].as_str().unwrap();
    let uri = format!("/api/admin/checkin/viewers/{id}/deductions");

    let (status, body) = call(
        &a.app,
        "POST",
        &uri,
        Some(serde_json::json!({"amount": 2, "reason": " 換一首歌 "})),
    )
    .await;
    assert_eq!((status, &body["available"]), (200, &1.into()));
    let (status, body) = call(&a.app, "POST", &uri, Some(serde_json::json!({"amount": 2}))).await;
    assert_eq!(status, 409);
    assert_eq!(
        body,
        serde_json::json!({"code": "checkin.not_enough_stamps", "available": 1})
    );
    for bad in [
        serde_json::json!({"amount": 0}),
        serde_json::json!({"amount": 1, "reason": "字".repeat(101)}),
    ] {
        let (status, body) = call(&a.app, "POST", &uri, Some(bad)).await;
        assert_eq!(
            (status, &body["code"]),
            (400, &"checkin.invalid_deduction".into())
        );
    }
    let (status, body) = call(
        &a.app,
        "POST",
        "/api/admin/checkin/viewers/nobody/deductions",
        Some(serde_json::json!({"amount": 1})),
    )
    .await;
    assert_eq!(
        (status, &body["code"]),
        (404, &"checkin.viewer_not_found".into())
    );

    let (_, page) = call(&a.app, "GET", "/api/admin/checkin/viewers", None).await;
    let ann = viewer_named(&page, "Ann");
    assert_eq!(
        (&ann["total"], &ann["available"], &ann["stamps"]),
        (&3.into(), &1.into(), &3.into()),
        "the total and the card stay"
    );

    let (status, log) = call(&a.app, "GET", "/api/admin/checkin/deductions", None).await;
    assert_eq!(status, 200);
    assert_eq!(log.as_array().unwrap().len(), 1);
    assert_eq!(
        (&log[0]["user_name"], &log[0]["amount"], &log[0]["reason"]),
        (&"Ann".into(), &2.into(), &"換一首歌".into())
    );
}

#[tokio::test]
async fn two_deductions_at_once_never_go_below_zero() {
    let a = api("deduct-race").await;
    seed(&a.pool, "1", "Ann", "2026-11-10T01:00:00Z").await;
    let (_, page) = call(&a.app, "GET", "/api/admin/checkin/viewers", None).await;
    let id = viewer_named(&page, "Ann")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let uri = format!("/api/admin/checkin/viewers/{id}/deductions");
    let body = Some(serde_json::json!({"amount": 1}));
    let (first, second) = tokio::join!(
        call(&a.app, "POST", &uri, body.clone()),
        call(&a.app, "POST", &uri, body.clone())
    );
    let mut statuses = [first.0, second.0];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);
    let (_, page) = call(&a.app, "GET", "/api/admin/checkin/viewers", None).await;
    assert_eq!(viewer_named(&page, "Ann")["available"], 0);
}

#[tokio::test]
async fn an_import_below_what_was_deducted_shows_zero_and_keeps_the_log() {
    let a = api("deduct-import").await;
    let file = serde_json::json!({ "1": { "totalCheckIns": 10, "name": "Ann" } }).to_string();
    post_raw(&a.app, "/api/admin/checkin/import", file).await;
    let (_, page) = call(&a.app, "GET", "/api/admin/checkin/viewers", None).await;
    let id = viewer_named(&page, "Ann")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, _) = call(
        &a.app,
        "POST",
        &format!("/api/admin/checkin/viewers/{id}/deductions"),
        Some(serde_json::json!({"amount": 8})),
    )
    .await;
    assert_eq!(status, 200);

    let file = serde_json::json!({ "1": { "totalCheckIns": 5, "name": "Ann" } }).to_string();
    let (status, _) = post_raw(&a.app, "/api/admin/checkin/import", file).await;
    assert_eq!(status, 200);
    let (_, page) = call(&a.app, "GET", "/api/admin/checkin/viewers", None).await;
    let ann = viewer_named(&page, "Ann");
    assert_eq!((&ann["total"], &ann["available"]), (&5.into(), &0.into()));
    let (_, log) = call(&a.app, "GET", "/api/admin/checkin/deductions", None).await;
    assert_eq!(log.as_array().unwrap().len(), 1);
}

async fn upload_image(app: &axum::Router, name: &str, bytes: &[u8]) -> (u16, serde_json::Value) {
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/admin/checkin/images?name={name}"))
        .header("content-type", "application/octet-stream")
        .body(Body::from(bytes.to_vec()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status().as_u16();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n rest";

#[tokio::test]
async fn the_card_style_is_checked_saved_and_its_images_served() {
    let a = api("card-style").await;
    let uri = "/api/admin/checkin/card-style";
    let (status, style) = call(&a.app, "GET", uri, None).await;
    assert_eq!(
        (status, &style["preset"], &style["title"]),
        (200, &"simple".into(), &"#觀眾#".into())
    );

    let (status, body) = upload_image(&a.app, "bg.svg", b"<svg onload=alert(1)>").await;
    assert_eq!(
        (status, &body["code"]),
        (400, &"checkin.unsupported_image".into())
    );
    let (status, body) = upload_image(&a.app, "bg.png", b"GIF89a").await;
    assert_eq!(
        (status, &body["code"]),
        (400, &"checkin.unsupported_image".into())
    );
    let (status, body) = upload_image(&a.app, "big.png", &vec![0; 5 * 1024 * 1024 + 1]).await;
    assert_eq!(
        (status, &body["code"]),
        (413, &"checkin.image_too_large".into())
    );
    let (status, image) = upload_image(&a.app, "bg.png", PNG).await;
    assert_eq!(status, 200);
    let name = image["name"].as_str().unwrap().to_owned();

    let mut next = style.clone();
    next["preset"] = "journal".into();
    next["background_image"] = name.clone().into();
    next["per_row"] = 5.into();
    next["title"] = " #觀眾# 打卡 ".into();
    let (status, saved) = call(&a.app, "PUT", uri, Some(next.clone())).await;
    assert_eq!(status, 200);
    assert_eq!(
        (&saved["title"], &saved["background_image"]),
        (&"#觀眾# 打卡".into(), &name.clone().into())
    );
    assert_eq!(
        toolkit_core::settings::load(&a.dir).checkin.card.preset,
        "journal"
    );

    for (field, value) in [
        ("preset", serde_json::json!("fancy")),
        ("background_color", serde_json::json!("red")),
        ("per_row", serde_json::json!(0)),
        (
            "stamp_image",
            serde_json::json!("0190a1b2-0000-7000-8000-000000000009.png"),
        ),
    ] {
        let mut bad = saved.clone();
        bad[field] = value;
        let (status, body) = call(&a.app, "PUT", uri, Some(bad)).await;
        assert_eq!(
            (status, &body["code"]),
            (400, &"checkin.invalid_style".into()),
            "{field}"
        );
    }

    let files = super::super::card_style::image_routes(a.service.clone());
    let res = files
        .clone()
        .oneshot(
            Request::get(format!("/checkin/{name}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["content-type"], "image/png");
    let res = files
        .oneshot(
            Request::get("/checkin/..%2Fsettings.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), 404);

    // A saved style without the image removes it from the folder.
    let mut plain = saved.clone();
    plain["background_image"] = serde_json::Value::Null;
    call(&a.app, "PUT", uri, Some(plain)).await;
    assert!(!a.dir.join("checkin").join(&name).exists());
}

#[tokio::test]
async fn a_test_card_records_nothing() {
    let a = api("card-test").await;
    let (status, _) = call(&a.app, "POST", "/api/admin/checkin/card-style/test", None).await;
    assert_eq!(status, 204);
    assert_eq!(count(&a.pool).await, 0);
    let recent = a.service.recent();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].source, Source::Simulation);
}

#[tokio::test]
async fn a_pack_preset_needs_the_pack_installed() {
    let a = api("card-pack").await;
    let uri = "/api/admin/checkin/card-style";
    let (_, mut style) = call(&a.app, "GET", uri, None).await;
    let id = "0190a1b2-0000-7000-8000-000000000001";
    style["preset"] = format!("pack:{id}").into();
    let (status, body) = call(&a.app, "PUT", uri, Some(style.clone())).await;
    assert_eq!(
        (status, &body["code"]),
        (400, &"checkin.invalid_style".into())
    );
    let pack = a.dir.join("templates").join("checkin").join(id);
    std::fs::create_dir_all(&pack).unwrap();
    std::fs::write(pack.join("index.html"), "<div data-card-stage></div>").unwrap();
    let (status, body) = call(&a.app, "PUT", uri, Some(style)).await;
    assert_eq!(
        (status, &body["preset"]),
        (200, &format!("pack:{id}").into())
    );
}

#[tokio::test]
async fn the_month_starts_with_the_business_day() {
    let a = api("month-start").await;
    let utc = |s: &str| s.parse::<Timestamp>().unwrap();
    // Asia/Taipei, reset at 0: 11/22 09:00 local belongs to November, which began 10/31 16:00 UTC.
    assert_eq!(
        a.service.month_start(utc("2026-11-22T01:00:00Z")),
        utc("2026-10-31T16:00:00Z")
    );
    // 11/01 07:00 local with a reset hour of 8 still belongs to October 31.
    call(
        &a.app,
        "PUT",
        "/api/admin/checkin/settings",
        Some(serde_json::json!({
            "command": "打卡", "allowed": "everyone", "time_zone": "Asia/Taipei", "reset_hour": 8,
            "card_slots": 10, "reward_id": null
        })),
    )
    .await;
    assert_eq!(
        a.service.month_start(utc("2026-10-31T23:00:00Z")),
        utc("2026-10-01T00:00:00Z")
    );
}
