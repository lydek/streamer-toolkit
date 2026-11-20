use super::*;

#[tokio::test]
async fn simulated_chat_validates_text() {
    let send = |body: &'static str| {
        get_req("/api/admin/simulate/chat", "127.0.0.1:47810")
            .method(Method::POST)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap()
    };
    assert_eq!(
        status(send(r#"{"text":"  "}"#)).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        status(send(r#"{"text":"hi","userName":"Tester"}"#)).await,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn the_chat_log_lists_streams_and_the_chat_between_and_one_periods_messages_and_events() {
    let mut config = test_config();
    config.db = db::memory_migrated().await;
    let pool = config.db.clone();
    let app = router(config).unwrap();
    for sql in [
        "INSERT INTO stream_sessions (id, twitch_stream_id, started_at, ended_at) VALUES              ('a', 'ta', '2026-11-18T10:00:00.000000Z', '2026-11-18T12:00:00.000000Z'),              ('b', 'tb', '2026-11-19T10:00:00.000000Z', '2026-11-19T12:00:00.000000Z')",
        "INSERT INTO chat_messages (id, twitch_message_id, session_id, user_id, payload, sent_at) VALUES              ('1', 'before', NULL, 'u', '{\"userId\":\"u\",\"userLogin\":\"u\",\"userName\":\"U\",\"color\":null,\"text\":\"x\",\"fragments\":[],\"badges\":[],\"highlighted\":false}', '2026-11-18T09:00:00.000000Z'),              ('2', 'in-a', 'a', 'u', '{\"userId\":\"u\",\"userLogin\":\"u\",\"userName\":\"U\",\"color\":null,\"text\":\"x\",\"fragments\":[],\"badges\":[],\"highlighted\":false}', '2026-11-18T11:00:00.000000Z'),              ('3', 'between', 'a', 'u', '{\"userId\":\"u\",\"userLogin\":\"u\",\"userName\":\"U\",\"color\":null,\"text\":\"x\",\"fragments\":[],\"badges\":[],\"highlighted\":false}', '2026-11-18T20:00:00.000000Z'),              ('4', 'in-b', 'b', 'u', '{\"userId\":\"u\",\"userLogin\":\"u\",\"userName\":\"U\",\"color\":null,\"text\":\"hi\",\"fragments\":[],\"badges\":[],\"highlighted\":false}', '2026-11-19T11:00:00.000000Z'),              ('5', 'after', 'b', 'u', '{\"userId\":\"u\",\"userLogin\":\"u\",\"userName\":\"U\",\"color\":null,\"text\":\"x\",\"fragments\":[],\"badges\":[],\"highlighted\":false}', '2026-11-19T13:00:00.000000Z')",
        "INSERT INTO activity_events (id, twitch_message_id, kind, user_name, gifted, session_id, occurred_at) VALUES              ('1', 'm1', 'follow', 'Fan', 0, 'b', '2026-11-19T10:30:00.000000Z'),              ('2', 'm2', 'subscribe', 'Lucky', 1, 'b', '2026-11-19T10:35:00.000000Z')",
        "INSERT INTO redemption_records (id, twitch_redemption_id, user_id, user_login, user_name, reward_id,              reward_title, reward_cost, user_input, status, redeemed_at, updated_at) VALUES              ('1', 't1', 'u', 'u', 'Kai', 'r', 'Water', 1, '', 'unfulfilled', '2026-11-19T10:40:00.000000Z', 'x')",
    ] {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }
    let get = |path: &str| {
        get_req(path, "127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap()
    };
    let json = |res: axum::response::Response| async move {
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap();
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
    };
    let periods = json(
        app.clone()
            .oneshot(get("/api/admin/chat/periods"))
            .await
            .unwrap(),
    )
    .await;
    let summary: Vec<(String, serde_json::Value, serde_json::Value, u64)> = periods
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["kind"].as_str().unwrap().to_owned(),
                p["from"].clone(),
                p["to"].clone(),
                p["messages"].as_u64().unwrap(),
            )
        })
        .collect();
    let t = |s: &str| serde_json::Value::String(s.into());
    let null = serde_json::Value::Null;
    assert_eq!(
        summary,
        [
            (
                "offline".into(),
                t("2026-11-19T12:00:00.000000Z"),
                null.clone(),
                1
            ),
            (
                "stream".into(),
                t("2026-11-19T10:00:00.000000Z"),
                t("2026-11-19T12:00:00.000000Z"),
                1
            ),
            (
                "offline".into(),
                t("2026-11-18T12:00:00.000000Z"),
                t("2026-11-19T10:00:00.000000Z"),
                1
            ),
            (
                "stream".into(),
                t("2026-11-18T10:00:00.000000Z"),
                t("2026-11-18T12:00:00.000000Z"),
                1
            ),
            ("offline".into(), null, t("2026-11-18T10:00:00.000000Z"), 1),
        ]
    );

    let log = json(
        app.clone()
            .oneshot(get(
                "/api/admin/chat/log?from=2026-11-19T10:00:00.000Z&to=2026-11-19T12:00:00.000Z",
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(log["messages"].as_array().unwrap().len(), 1);
    assert_eq!(log["messages"][0]["payload"]["text"], "hi");
    let events: Vec<(&str, &str)> = log["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["kind"].as_str().unwrap(), e["userName"].as_str().unwrap()))
        .collect();
    assert_eq!(
        events,
        [("follow", "Fan"), ("redemption", "Kai")],
        "a gifted sub is covered by its gift"
    );
    assert_eq!(log["events"][1]["reward"], "Water");
    assert_eq!(log["truncated"], false);

    let bad = app
        .clone()
        .oneshot(get("/api/admin/chat/log?from=yesterday"))
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    for path in ["/api/admin/chat/periods", "/api/admin/chat/log"] {
        let unauthorized = get_req(path, "127.0.0.1:47810")
            .body(Body::empty())
            .unwrap();
        assert_eq!(status(unauthorized).await, StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn the_chat_log_keeps_exact_bounds_and_says_when_anything_was_left_out() {
    const PAYLOAD: &str = r#"{"userId":"u","userLogin":"u","userName":"U","color":null,"text":"x","fragments":[],"badges":[],"highlighted":false}"#;
    let setup = || async {
        let mut config = test_config();
        config.db = db::memory_migrated().await;
        let pool = config.db.clone();
        (router(config).unwrap(), pool)
    };
    let fetch = |app: Router, path: String| async move {
        let req = get_req(&path, "127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 24)
            .await
            .unwrap();
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
    };
    // `n` streams of a minute each, an hour apart from 2026-11-01, and one message before the first.
    let streams = |pool: sqlx::SqlitePool, n: u32| async move {
        sqlx::query(
            "WITH RECURSIVE i(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM i WHERE n < ?) \
             INSERT INTO stream_sessions (id, twitch_stream_id, started_at, ended_at) \
             SELECT n, 't' || n, strftime('%Y-%m-%dT%H:%M:%S.000000Z', '2026-11-01', '+' || n || ' hours'), \
             strftime('%Y-%m-%dT%H:%M:%S.000000Z', '2026-11-01', '+' || n || ' hours', '+1 minutes') FROM i",
        )
        .bind(n)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_messages (id, twitch_message_id, user_id, payload, sent_at) \
             VALUES ('early', 'early', 'u', ?, '2026-10-31T00:00:00.000000Z')",
        )
        .bind(PAYLOAD)
        .execute(&pool)
        .await
        .unwrap();
    };
    for (n, listed, before_shown) in [(49, 49, true), (50, 50, true), (51, 50, false)] {
        let (app, pool) = setup().await;
        streams(pool, n).await;
        let periods = fetch(app, "/api/admin/chat/periods".into()).await;
        let periods = periods.as_array().unwrap();
        let kinds = |kind: &str| periods.iter().filter(|p| p["kind"] == kind).count();
        assert_eq!(kinds("stream"), listed, "{n} streams");
        assert_eq!(
            kinds("offline") == 1,
            before_shown,
            "{n} streams: the chat before the first shows only when no older stream was left out"
        );
    }

    // A message in the same millisecond as the stream's end, but before it.
    let (app, pool) = setup().await;
    sqlx::query(
        "INSERT INTO stream_sessions (id, twitch_stream_id, started_at, ended_at) \
         VALUES ('a', 'ta', '2026-11-19T10:00:00.000000Z', '2026-11-19T12:00:00.123900Z')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_messages (id, twitch_message_id, user_id, payload, sent_at) \
         VALUES ('1', 'edge', 'u', ?, '2026-11-19T12:00:00.123500Z')",
    )
    .bind(PAYLOAD)
    .execute(&pool)
    .await
    .unwrap();
    let periods = fetch(app.clone(), "/api/admin/chat/periods".into()).await;
    let stream = &periods[0];
    assert_eq!(
        (&stream["kind"], &stream["messages"]),
        (&serde_json::json!("stream"), &serde_json::json!(1))
    );
    assert_eq!(stream["to"], "2026-11-19T12:00:00.123900Z");
    let query = |p: &serde_json::Value| {
        format!(
            "/api/admin/chat/log?from={}&to={}",
            p["from"].as_str().unwrap(),
            p["to"].as_str().unwrap()
        )
    };
    let log = fetch(app.clone(), query(stream)).await;
    assert_eq!(
        log["messages"].as_array().unwrap().len(),
        1,
        "the period shows what it counts"
    );

    // Exactly a page of messages is complete; one more, or one event too many, is not.
    let (app, pool) = setup().await;
    let messages = |n: u32| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "WITH RECURSIVE i(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM i WHERE n < ?2) \
                 INSERT INTO chat_messages (id, twitch_message_id, user_id, payload, sent_at) \
                 SELECT 'm' || n, 'm' || n, 'u', ?1, strftime('%Y-%m-%dT%H:%M:%S.000000Z', '2026-11-19', '+' || n || ' seconds') \
                 FROM i WHERE NOT EXISTS (SELECT 1 FROM chat_messages WHERE id = 'm' || n)",
            )
            .bind(PAYLOAD)
            .bind(n)
            .execute(&pool)
            .await
            .unwrap();
        }
    };
    let log = |app: Router| fetch(app, "/api/admin/chat/log".into());
    messages(5000).await;
    let full = log(app.clone()).await;
    assert_eq!(full["messages"].as_array().unwrap().len(), 5000);
    assert_eq!(full["truncated"], false, "exactly a page");
    messages(5001).await;
    let over = log(app.clone()).await;
    assert_eq!(over["messages"].as_array().unwrap().len(), 5000);
    assert_eq!(over["truncated"], true);
    assert_eq!(over["messages"][0]["id"], "m2", "the latest are kept");

    let (app, pool) = setup().await;
    sqlx::query(
        "WITH RECURSIVE i(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM i WHERE n < 2001) \
         INSERT INTO activity_events (id, twitch_message_id, kind, user_name, gifted, occurred_at) \
         SELECT n, 'a' || n, 'follow', 'Fan', 0, strftime('%Y-%m-%dT%H:%M:%S.000000Z', '2026-11-19', '+' || n || ' seconds') FROM i",
    )
    .execute(&pool)
    .await
    .unwrap();
    let events = log(app).await;
    assert_eq!(events["events"].as_array().unwrap().len(), 2000);
    assert_eq!(events["truncated"], true, "events count too");
}

#[tokio::test]
async fn chat_display_settings_and_avatars_need_the_token_and_are_checked() {
    let dir = std::env::temp_dir().join(format!("st-host-chatdisplay-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut config = test_config();
    config.data_dir = dir.clone();
    config.db = db::memory_migrated().await;
    sqlx::query(
        "INSERT INTO viewers (twitch_user_id, login, display_name, avatar_url, first_seen_at, last_seen_at) VALUES              ('7', 'fan', 'Fan', 'https://static-cdn.jtvnw.net/a.png', 'x', 'x'), ('8', 'kai', 'Kai', NULL, 'x', 'x')",
    )
    .execute(&config.db)
    .await
    .unwrap();
    let app = router(config).unwrap();
    let call = |method: Method, uri: &str, body: Option<&'static str>, token: bool| {
        let mut b = get_req(uri, "127.0.0.1:47810")
            .method(method)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .header(header::CONTENT_TYPE, "application/json");
        if token {
            b = b.header(header::AUTHORIZATION, format!("Bearer {TOKEN}"));
        }
        b.body(body.map_or_else(Body::empty, Body::from)).unwrap()
    };
    let json = |res: axum::response::Response| async move {
        let bytes = axum::body::to_bytes(res.into_body(), 65536).await.unwrap();
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
    };
    let res = app
        .clone()
        .oneshot(call(Method::GET, "/api/admin/chat-display", None, true))
        .await
        .unwrap();
    assert_eq!(json(res).await, serde_json::Value::Null, "never changed");
    let good = r#"{"style":"compact","avatars":false,"badges":true,"time":true,"textSize":120}"#;
    let res = app
        .clone()
        .oneshot(call(
            Method::PUT,
            "/api/admin/chat-display",
            Some(good),
            true,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = app
        .clone()
        .oneshot(call(Method::GET, "/api/admin/chat-display", None, true))
        .await
        .unwrap();
    assert_eq!(
        json(res).await,
        serde_json::from_str::<serde_json::Value>(good).unwrap()
    );
    for bad in [
        r#"{"style":"compact","avatars":false,"badges":true,"time":true,"textSize":125}"#,
        r#"{"style":"huge","avatars":false,"badges":true,"time":true,"textSize":100}"#,
    ] {
        let res = app
            .clone()
            .oneshot(call(
                Method::PUT,
                "/api/admin/chat-display",
                Some(bad),
                true,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    let res = app
        .clone()
        .oneshot(call(
            Method::GET,
            "/api/admin/viewers/avatars?ids=7,8,9",
            None,
            true,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        json(res).await,
        serde_json::json!({"7": "https://static-cdn.jtvnw.net/a.png"})
    );
    let res = app
        .clone()
        .oneshot(call(
            Method::GET,
            "/api/admin/viewers/avatars?ids=7,x%27",
            None,
            true,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let res = app
        .clone()
        .oneshot(call(Method::GET, "/api/admin/chat/badges", None, true))
        .await
        .unwrap();
    assert_eq!(
        json(res).await,
        serde_json::json!([]),
        "signed out: no badges"
    );

    for (method, uri) in [
        (Method::GET, "/api/admin/chat-display"),
        (Method::GET, "/api/admin/viewers/avatars?ids=7"),
        (Method::GET, "/api/admin/chat/badges"),
    ] {
        let res = app
            .clone()
            .oneshot(call(method, uri, None, false))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
    let _ = std::fs::remove_dir_all(dir);
}
