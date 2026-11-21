use super::*;

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
    let at = DateTime::parse_from_rfc3339(at).unwrap().to_utc();
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
    let now = Utc::now();
    let today = a.service.config().rules().day.day_of(now);
    seed(&a.pool, "1", "Ann", &now.to_rfc3339()).await;
    seed(
        &a.pool,
        "2",
        "Bob",
        &(now + chrono::Duration::milliseconds(1)).to_rfc3339(),
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
        .day_of(Utc::now())
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
