use super::*;

#[tokio::test]
async fn checkin_can_be_closed_and_its_dead_letters_handled() {
    let dir = std::env::temp_dir().join(format!("st-host-close-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut config = test_config();
    config.data_dir = dir.clone();
    config.db = db::open(&dir).await.unwrap();
    let app = router(config).unwrap();
    let call = |method: Method, uri: &str, body: Option<serde_json::Value>| {
        let mut b = get_req(uri, "127.0.0.1:47810")
            .method(method)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"));
        if body.is_some() {
            b = b.header(header::CONTENT_TYPE, "application/json");
        }
        let req = b
            .body(body.map_or_else(Body::empty, |v| Body::from(v.to_string())))
            .unwrap();
        let app = app.clone();
        async move {
            let res = app.oneshot(req).await.unwrap();
            let status = res.status();
            let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap();
            (
                status,
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_default(),
            )
        }
    };
    let close = serde_json::json!({ "enabled": false, "close": true });
    let (status, _) = call(Method::PUT, "/api/admin/modules/checkin", Some(close)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, list) = call(Method::GET, "/api/admin/modules", None).await;
    let checkin = list
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == "checkin")
        .unwrap();
    assert_eq!(
        (&checkin["enabled"], &checkin["closed"]),
        (&false.into(), &true.into())
    );

    let enable = serde_json::json!({ "enabled": true });
    let (status, _) = call(Method::PUT, "/api/admin/modules/checkin", Some(enable)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, outbox) = call(Method::GET, "/api/admin/checkin/outbox", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        outbox,
        serde_json::json!({ "pending": 0, "oldest_pending_at": null, "dead_letters": [] })
    );
    for action in ["retry", "skip"] {
        let (status, body) = call(
            Method::POST,
            &format!("/api/admin/checkin/outbox/nope/{action}"),
            None,
        )
        .await;
        assert_eq!(
            (status, &body["code"]),
            (StatusCode::NOT_FOUND, &"outbox.not_found".into())
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn redemption_records_need_the_token() {
    let dir = std::env::temp_dir().join(format!("st-host-redemption-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut config = test_config();
    config.db = db::open(&dir).await.unwrap();
    let app = router(config).unwrap();
    let req = |token: Option<&str>| {
        let b = get_req("/api/admin/redemptions", "127.0.0.1:47810");
        match token {
            Some(t) => b.header(header::AUTHORIZATION, format!("Bearer {t}")),
            None => b,
        }
        .body(Body::empty())
        .unwrap()
    };
    for token in [None, Some("nope")] {
        let res = app.clone().oneshot(req(token)).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
    let res = app.clone().oneshot(req(Some(TOKEN))).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({ "items": [], "total": 0, "failed_writes": 0 })
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn sound_library_needs_the_token_but_sound_files_do_not() {
    for (token, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(TOKEN), StatusCode::OK),
    ] {
        let b = get_req("/api/admin/sounds", "127.0.0.1:47810");
        let b = match token {
            Some(t) => b.header(header::AUTHORIZATION, format!("Bearer {t}")),
            None => b,
        };
        assert_eq!(status(b.body(Body::empty()).unwrap()).await, expected);
    }
    let upload = Request::post("/api/admin/sounds?name=a.wav")
        .header(header::HOST, "127.0.0.1:47810")
        .header(header::ORIGIN, "http://127.0.0.1:47810")
        .body(Body::from(&b"RIFF\0\0\0\0WAVE"[..]))
        .unwrap();
    assert_eq!(status(upload).await, StatusCode::UNAUTHORIZED);
    // The file route is public (overlays have no token) and answers unknown IDs with 404, not 401.
    let file = get_req(
        "/sounds/0190a5b0-0000-7000-8000-000000000000",
        "127.0.0.1:47810",
    )
    .body(Body::empty())
    .unwrap();
    assert_eq!(status(file).await, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn music_library_needs_the_token_but_music_files_do_not() {
    for (token, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(TOKEN), StatusCode::OK),
    ] {
        let b = get_req("/api/admin/music", "127.0.0.1:47810");
        let b = match token {
            Some(t) => b.header(header::AUTHORIZATION, format!("Bearer {t}")),
            None => b,
        };
        assert_eq!(status(b.body(Body::empty()).unwrap()).await, expected);
    }
    let upload = Request::post("/api/admin/music?name=a.wav")
        .header(header::HOST, "127.0.0.1:47810")
        .header(header::ORIGIN, "http://127.0.0.1:47810")
        .body(Body::from(&b"RIFF\0\0\0\0WAVE"[..]))
        .unwrap();
    assert_eq!(status(upload).await, StatusCode::UNAUTHORIZED);
    // The file route is public (the music overlay has no token) and answers unknown IDs with 404, not 401.
    let file = get_req(
        "/music/0190a5b0-0000-7000-8000-000000000000",
        "127.0.0.1:47810",
    )
    .body(Body::empty())
    .unwrap();
    assert_eq!(status(file).await, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn checkin_settings_need_the_token() {
    for (token, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some("nope"), StatusCode::UNAUTHORIZED),
        (Some(TOKEN), StatusCode::OK),
    ] {
        let b = get_req("/api/admin/checkin/settings", "127.0.0.1:47810");
        let b = match token {
            Some(t) => b.header(header::AUTHORIZATION, format!("Bearer {t}")),
            None => b,
        };
        assert_eq!(status(b.body(Body::empty()).unwrap()).await, expected);
    }
}

#[tokio::test]
async fn simulated_check_ins_are_validated() {
    let app = app();
    for (body, expected) in [
        (r#"{"userName":"Sim","stamps":10}"#, (204, None)),
        (r#"{"stamps":0}"#, (400, Some("simulate.invalid_stamps"))),
        (r#"{"stamps":11}"#, (400, Some("simulate.invalid_stamps"))),
        (
            r#"{"userName":"Sim"}"#,
            (400, Some("simulate.invalid_checkin")),
        ),
    ] {
        let req = get_req("/api/admin/simulate/checkin", "127.0.0.1:47810")
            .method(Method::POST)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let code = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|v| v["code"].as_str().map(str::to_owned));
        assert_eq!((status, code.as_deref()), expected, "{body}");
    }
}

#[tokio::test]
async fn checkin_rewards_need_a_sign_in() {
    let req = get_req("/api/admin/checkin/rewards", "127.0.0.1:47810")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let res = app().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["code"],
        "twitch.sign_in_required"
    );
}

#[tokio::test]
async fn recent_alerts_list_a_simulated_one_and_replay_it_with_the_token() {
    let app = router(test_config()).unwrap();
    let req = |method: Method, path: &str, body: Option<&'static str>, token: bool| {
        let mut b = get_req(path, "127.0.0.1:47810")
            .method(method)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .header(header::CONTENT_TYPE, "application/json");
        if token {
            b = b.header(header::AUTHORIZATION, format!("Bearer {TOKEN}"));
        }
        b.body(body.map_or_else(Body::empty, Body::from)).unwrap()
    };
    let sim = req(
        Method::POST,
        "/api/admin/simulate/alert",
        Some(r#"{"kind":"follow","userName":"Fan"}"#),
        true,
    );
    assert_eq!(
        app.clone().oneshot(sim).await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
    let res = app
        .clone()
        .oneshot(req(Method::GET, "/api/admin/alerts/recent", None, true))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 65536).await.unwrap();
    let recent: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(recent[0]["payload"]["userName"], "Fan");
    assert_eq!(recent[0]["source"], "simulation");
    let id = recent[0]["id"].as_str().unwrap().to_owned();

    let replay = format!("/api/admin/alerts/recent/{id}/replay");
    let res = app
        .clone()
        .oneshot(req(Method::POST, &replay, None, true))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let gone = req(
        Method::POST,
        "/api/admin/alerts/recent/nope/replay",
        None,
        true,
    );
    assert_eq!(
        app.clone().oneshot(gone).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
    for (method, path) in [
        (Method::GET, "/api/admin/alerts/recent"),
        (Method::POST, replay.as_str()),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(req(method, path, None, false))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
}

#[tokio::test]
async fn simulated_alert_answers_bad_bodies_with_an_error_code() {
    let send = |body: &'static str| {
        get_req("/api/admin/simulate/alert", "127.0.0.1:47810")
            .method(Method::POST)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap()
    };
    for (body, code) in [
        (r#"{"kind":"nope"}"#, Some("simulate.invalid_alert")),
        (
            r#"{"kind":"raid","amount":"many"}"#,
            Some("simulate.invalid_alert"),
        ),
        (
            r#"{"kind":"raid","amount":0}"#,
            Some("simulate.invalid_amount"),
        ),
        (r#"{"kind":"raid","amount":5}"#, None),
    ] {
        let res = app().oneshot(send(body)).await.unwrap();
        match code {
            Some(code) => {
                assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{body}");
                let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
                    .await
                    .unwrap();
                let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(json["code"], code, "{body}");
            }
            None => assert_eq!(res.status(), StatusCode::NO_CONTENT, "{body}"),
        }
    }
}

#[tokio::test]
async fn skipping_clearing_and_stopping_the_queue_need_the_token() {
    let post = |path: &str, token: bool| {
        let mut b = get_req(path, "127.0.0.1:47810")
            .method(Method::POST)
            .header(header::ORIGIN, "http://127.0.0.1:47810");
        if token {
            b = b.header(header::AUTHORIZATION, format!("Bearer {TOKEN}"));
        }
        b.body(Body::empty()).unwrap()
    };
    let body = |res: axum::response::Response| async move {
        let bytes = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
    };
    let res = app()
        .oneshot(post("/api/admin/queue/clear", true))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body(res).await, serde_json::json!({"cancelled": 0}));
    let res = app()
        .oneshot(post("/api/admin/queue/stop", true))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        body(res).await,
        serde_json::json!({"stopped": false, "cancelled": 0})
    );
    let res = app()
        .oneshot(post("/api/admin/queue/skip", true))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body(res).await, serde_json::json!({"skipped": false}));
    for path in [
        "/api/admin/queue/skip",
        "/api/admin/queue/clear",
        "/api/admin/queue/stop",
    ] {
        assert_eq!(status(post(path, false)).await, StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn overlay_counts_need_the_token() {
    let res = app()
        .oneshot(
            get_req("/api/admin/overlays", "127.0.0.1:47810")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({"chat": 0, "alerts": 0, "checkin": 0, "sound": 0, "bgm": 0, "other": 0})
    );
    let unauthorized = get_req("/api/admin/overlays", "127.0.0.1:47810")
        .body(Body::empty())
        .unwrap();
    assert_eq!(status(unauthorized).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_stream_summary_adds_up_the_latest_session_and_needs_the_token() {
    let mut config = test_config();
    config.db = db::memory_migrated().await;
    let pool = config.db.clone();
    let app = router(config).unwrap();
    let req = |token: bool| {
        let mut b = get_req("/api/admin/stream/summary", "127.0.0.1:47810");
        if token {
            b = b.header(header::AUTHORIZATION, format!("Bearer {TOKEN}"));
        }
        b.body(Body::empty()).unwrap()
    };
    let body = |res: axum::response::Response| async move {
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
    };
    assert_eq!(
        body(app.clone().oneshot(req(true)).await.unwrap()).await,
        serde_json::Value::Null,
        "before the first stream"
    );

    // A stream that ended after this Host started, one follow and one redemption in it; an older stream's
    // follow and a redemption before the start do not count.
    let started = "2026-11-19T10:00:00.000000Z";
    let ended = (chrono::Utc::now() + chrono::TimeDelta::hours(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    sqlx::query(
        "INSERT INTO stream_sessions (id, twitch_stream_id, started_at, ended_at, peak_viewers) VALUES              ('old', 'a', '2026-11-18T10:00:00.000000Z', '2026-11-18T12:00:00.000000Z', 9), ('s1', 'b', ?, ?, 58)",
    )
    .bind(started)
    .bind(ended)
    .execute(&pool)
    .await
    .unwrap();
    for sql in [
        "INSERT INTO activity_events (id, twitch_message_id, kind, session_id, occurred_at) VALUES              ('1', 'm1', 'follow', 's1', '2026-11-19T10:05:00.000000Z'),              ('2', 'm2', 'follow', 'old', '2026-11-18T10:05:00.000000Z')",
        "INSERT INTO redemption_records (id, twitch_redemption_id, user_id, user_login, user_name, reward_id,              reward_title, reward_cost, user_input, status, redeemed_at, updated_at) VALUES              ('1', 't1', 'u', 'u', 'u', 'r', 'r', 1, '', 'unfulfilled', '2026-11-19T10:30:00.000000Z', 'x'),              ('2', 't2', 'u', 'u', 'u', 'r', 'r', 1, '', 'unfulfilled', '2026-11-19T09:30:00.000000Z', 'x')",
        "INSERT INTO chat_messages (id, twitch_message_id, session_id, user_id, payload, first_chat, sent_at) VALUES              ('1', 'c1', 's1', 'u', '{}', 1, '2026-11-19T10:06:00.000000Z'),              ('2', 'c2', 'old', 'u', '{}', 1, '2026-11-18T10:06:00.000000Z')",
    ] {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }
    let summary = body(app.clone().oneshot(req(true)).await.unwrap()).await;
    assert_eq!(summary["startedAt"], "2026-11-19T10:00:00.000Z");
    assert_eq!(summary["endedSinceLaunch"], true);
    assert_eq!(summary["peakViewers"], 58);
    assert_eq!(summary["follows"], 1);
    assert_eq!(summary["subs"], 0);
    assert_eq!(summary["redemptions"], 1);
    assert_eq!(summary["chatMessages"], 1);
    assert_eq!(summary["firstChats"], 1);
    assert_eq!(
        status(req(false)).await,
        StatusCode::UNAUTHORIZED,
        "the token is required"
    );
}
