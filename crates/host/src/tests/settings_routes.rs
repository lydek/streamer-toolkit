use super::*;

#[tokio::test]
async fn invalid_client_id_is_rejected() {
    let req = get_req("/api/admin/twitch/client-id", "127.0.0.1:47810")
        .method(Method::PUT)
        .header(header::ORIGIN, "http://127.0.0.1:47810")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"client_id":"<script>"}"#))
        .unwrap();
    assert_eq!(status(req).await, StatusCode::BAD_REQUEST);
}

/// One secret slot that remembers deletes, for tests that need a stored Twitch token.
#[derive(Default)]
struct MemSecrets(std::sync::Mutex<Option<String>>);
impl SecretStore for MemSecrets {
    fn read(&self) -> Result<Option<String>, SecretStoreError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn write(&self, v: &str) -> Result<(), SecretStoreError> {
        *self.0.lock().unwrap() = Some(v.into());
        Ok(())
    }
    fn delete(&self) -> Result<(), SecretStoreError> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

fn put_client_id(id: &str) -> Request {
    get_req("/api/admin/twitch/client-id", "127.0.0.1:47810")
        .method(Method::PUT)
        .header(header::ORIGIN, "http://127.0.0.1:47810")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(format!(r#"{{"client_id":"{id}"}}"#)))
        .unwrap()
}

#[tokio::test]
async fn saving_the_same_client_id_keeps_the_sign_in() {
    let dir = std::env::temp_dir().join(format!("st-host-same-id-{}", std::process::id()));
    settings::update(&dir, |s| s.twitch_client_id = "abc".into()).unwrap();
    let secrets = Arc::new(MemSecrets::default());
    *secrets.0.lock().unwrap() = Some(
        r#"{"client_id":"abc","access_token":"a","refresh_token":"r","user_id":"42","login":"streamer","scopes":[]}"#.into(),
    );
    let app = router(HostConfig {
        port: 47810,
        extra_origins: Vec::new(),
        admin_token: TOKEN.into(),
        data_dir: dir.clone(),
        twitch_secrets: secrets.clone(),
        twitch: TwitchEndpoints {
            oauth: "http://127.0.0.1:1".into(),
            helix: "http://127.0.0.1:1".into(),
            eventsub: "ws://127.0.0.1:1".into(),
            irc: "ws://127.0.0.1:1".into(),
        },
        obs_secrets: Arc::new(NoSecrets),
        alert_timing: AlertTiming::default(),
        checkin_timing: CheckinTiming::default(),
        presentation_gap: PRESENTATION_GAP,
        db: db::memory(),
        app_version: "0.0.0-test".into(),
        latest_release_url: None,
        restart: Arc::new(|| {}),
    })
    .unwrap();

    let res = app.clone().oneshot(put_client_id("abc")).await.unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert!(
        secrets.0.lock().unwrap().is_some(),
        "unchanged ID keeps the token"
    );

    let res = app.clone().oneshot(put_client_id("xyz")).await.unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert!(secrets.0.lock().unwrap().is_none(), "a new ID signs out");
    assert_eq!(settings::load(&dir).twitch_client_id, "xyz");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn obs_commands_need_a_connection_and_something_to_change() {
    let dir = std::env::temp_dir().join(format!("st-host-obs-cmd-{}", std::process::id()));
    // Port 1 refuses at once, so a running OBS on this machine cannot answer.
    settings::update(&dir, |s| s.obs.port = 1).unwrap();
    let mut config = test_config();
    config.data_dir = dir.clone();
    let app = router(config).unwrap();
    let put = |path: &'static str, body: &'static str| {
        get_req(path, "127.0.0.1:47810")
            .method(Method::PUT)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap()
    };
    let code = |res: axum::response::Response| async move {
        let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["code"].clone()
    };

    let res = app
        .clone()
        .oneshot(put("/api/admin/obs/scene", r#"{"scene":"Live"}"#))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(code(res).await, "obs.not_connected");
    let res = app
        .clone()
        .oneshot(put(
            "/api/admin/obs/item",
            r#"{"scene":"Live","item":1,"enabled":false}"#,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let res = app
        .oneshot(put("/api/admin/obs/item", r#"{"scene":"Live","item":1}"#))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(code(res).await, "obs.nothing_to_change");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn obs_password_goes_to_the_secret_store_only() {
    let dir = std::env::temp_dir().join(format!("st-host-obs-{}", std::process::id()));
    let secrets = Arc::new(MemSecrets::default());
    let mut config = test_config();
    config.data_dir = dir.clone();
    config.obs_secrets = secrets.clone();
    let app = router(config).unwrap();
    let put = |body: &'static str| {
        get_req("/api/admin/obs", "127.0.0.1:47810")
            .method(Method::PUT)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap()
    };

    let res = app
        .clone()
        .oneshot(put(
            r#"{"host":"obs-pc.local","port":4456,"password":"hunter2"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(secrets.0.lock().unwrap().as_deref(), Some("hunter2"));
    let saved = std::fs::read_to_string(settings::path(&dir)).unwrap();
    assert!(saved.contains("obs-pc.local"));
    assert!(!saved.contains("hunter2"), "{saved}");

    let res = app
        .clone()
        .oneshot(
            get_req("/api/admin/obs", "127.0.0.1:47810")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
    let overview: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(overview["port"], 4456);
    assert_eq!(overview["password_set"], true);
    assert!(!String::from_utf8_lossy(&body).contains("hunter2"));

    // Without `password` the stored one stays; an empty one removes it.
    let res = app
        .clone()
        .oneshot(put(r#"{"host":"127.0.0.1","port":4455}"#))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert!(secrets.0.lock().unwrap().is_some());
    let res = app
        .clone()
        .oneshot(put(r#"{"host":"127.0.0.1","port":4455,"password":""}"#))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert!(secrets.0.lock().unwrap().is_none());

    let res = app
        .clone()
        .oneshot(put(r#"{"host":"ws://evil","port":4455}"#))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let unauthorized = get_req("/api/admin/obs", "127.0.0.1:47810")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.oneshot(unauthorized).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn ask_before_close_is_the_inverse_of_skip_close_hint() {
    let dir = std::env::temp_dir().join(format!("st-host-general-{}", std::process::id()));
    settings::update(&dir, |s| s.skip_close_hint = true).unwrap();
    let mut config = test_config();
    config.data_dir = dir.clone();
    let app = router(config).unwrap();
    let get = || {
        get_req("/api/admin/general", "127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap()
    };
    let read = |res: axum::response::Response| async {
        let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["ask_before_close"].clone()
    };

    assert_eq!(read(app.clone().oneshot(get()).await.unwrap()).await, false);
    let put = get_req("/api/admin/general", "127.0.0.1:47810")
        .method(Method::PUT)
        .header(header::ORIGIN, "http://127.0.0.1:47810")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"ask_before_close":true}"#))
        .unwrap();
    let res = app.clone().oneshot(put).await.unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert!(!settings::load(&dir).skip_close_hint);
    assert_eq!(read(app.oneshot(get()).await.unwrap()).await, true);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn broken_settings_are_reported_and_never_saved_over() {
    let dir = std::env::temp_dir().join(format!("st-host-broken-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let broken = r#"{"skip_close_hint":tru"#;
    std::fs::write(settings::path(&dir), broken).unwrap();
    let mut config = test_config();
    config.data_dir = dir.clone();
    let app = router(config).unwrap();
    let get = get_req("/api/admin/general", "127.0.0.1:47810")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let body = axum::body::to_bytes(app.clone().oneshot(get).await.unwrap().into_body(), 4096)
        .await
        .unwrap();
    let general: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(general["settings_unreadable"], true);

    let put = get_req("/api/admin/general", "127.0.0.1:47810")
        .method(Method::PUT)
        .header(header::ORIGIN, "http://127.0.0.1:47810")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"ask_before_close":false}"#))
        .unwrap();
    let res = app.oneshot(put).await.unwrap();
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        std::fs::read_to_string(settings::path(&dir)).unwrap(),
        broken
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn module_switch_rejects_unknown_modules() {
    let req = get_req("/api/admin/modules/nope", "127.0.0.1:47810")
        .method(Method::PUT)
        .header(header::ORIGIN, "http://127.0.0.1:47810")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"enabled":false}"#))
        .unwrap();
    assert_eq!(status(req).await, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn home_tiles_persist_and_reject_bad_bodies() {
    let dir = std::env::temp_dir().join(format!("st-host-tiles-{}", std::process::id()));
    let app_for = |dir: &PathBuf| {
        let mut config = test_config();
        config.data_dir = dir.clone();
        router(config).unwrap()
    };
    let put = |body: &str| {
        get_req("/api/admin/home-tiles", "127.0.0.1:47810")
            .method(Method::PUT)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap()
    };
    let get = |app: Router| async move {
        let res = app
            .oneshot(
                get_req("/api/admin/home-tiles", "127.0.0.1:47810")
                    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()
    };

    // A list saved while home grouped its tiles comes back once in the order it was shown: 「直播中」 first, and
    // the stage blocks, which always showed above the tiles, before them.
    let blocks = ["pending", "checklist", "stats", "summary", "queue"];
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("settings.json"),
        r#"{"home_tiles":["checkin","chat","sound","feed"]}"#,
    )
    .unwrap();
    let mut expected: Vec<&str> = blocks.to_vec();
    expected.extend(["chat", "feed", "checkin", "sound"]);
    assert_eq!(get(app_for(&dir)).await, serde_json::json!(expected));
    // A one-grid list (version 1), even an emptied one, only gets the blocks.
    std::fs::write(
        dir.join("settings.json"),
        r#"{"home_tiles":[],"home_tiles_version":1}"#,
    )
    .unwrap();
    assert_eq!(get(app_for(&dir)).await, serde_json::json!(blocks));
    // A full old list stays within what PUT takes, so it can be saved back as read.
    let full: Vec<String> = (0..50).map(|n| format!("t{n}")).collect();
    std::fs::write(
        dir.join("settings.json"),
        serde_json::json!({ "home_tiles": full, "home_tiles_version": 1 }).to_string(),
    )
    .unwrap();
    let read = get(app_for(&dir)).await;
    assert_eq!(read.as_array().unwrap().len(), 50);
    assert_eq!(read[0], "pending");
    assert_eq!(read[49], "t44");
    let res = app_for(&dir).oneshot(put(&read.to_string())).await.unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = app_for(&dir)
        .oneshot(put(r#"["checkin","chat"]"#))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        get(app_for(&dir)).await,
        serde_json::json!(["checkin", "chat"]),
        "a flat order is kept as saved"
    );
    std::fs::remove_file(dir.join("settings.json")).unwrap();

    let app = app_for(&dir);
    assert_eq!(get(app.clone()).await, serde_json::Value::Null);
    let res = app.clone().oneshot(put("[]")).await.unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    // A new router on the same data folder stands in for a restart.
    assert_eq!(get(app_for(&dir)).await, serde_json::json!([]));

    let res = app
        .clone()
        .oneshot(put(r#"["chat","chat"]"#))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(get(app.clone()).await, serde_json::json!(["chat"]));

    let res = app.clone().oneshot(put("null")).await.unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(get(app.clone()).await, serde_json::Value::Null);
    let saved = std::fs::read_to_string(settings::path(&dir)).unwrap();
    assert!(!saved.contains("home_tiles"), "{saved}");

    let too_many = serde_json::to_string(&vec!["a"; 51]).unwrap();
    for bad in [r#"["Chat"]"#, r#"[1]"#, "{}", "not json", too_many.as_str()] {
        let res = app.clone().oneshot(put(bad)).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{bad}");
        let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["code"], "home.invalid_tiles", "{bad}");
    }
    assert_eq!(
        get(app.clone()).await,
        serde_json::Value::Null,
        "bad bodies change nothing"
    );
    let unauthorized = get_req("/api/admin/home-tiles", "127.0.0.1:47810")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.oneshot(unauthorized).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn text_size_persists_and_rejects_bad_bodies() {
    let dir = std::env::temp_dir().join(format!("st-host-textsize-{}", std::process::id()));
    let app_for = |dir: &PathBuf| {
        let mut config = test_config();
        config.data_dir = dir.clone();
        router(config).unwrap()
    };
    let put = |body: &str| {
        get_req("/api/admin/text-size", "127.0.0.1:47810")
            .method(Method::PUT)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap()
    };
    let get = |app: Router| async move {
        let res = app
            .oneshot(
                get_req("/api/admin/text-size", "127.0.0.1:47810")
                    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()
    };

    let app = app_for(&dir);
    assert_eq!(get(app.clone()).await, serde_json::Value::Null);
    let res = app.clone().oneshot(put("130")).await.unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    // A new router on the same data folder stands in for a restart.
    assert_eq!(get(app_for(&dir)).await, serde_json::json!(130));
    for bad in ["80", "160", "105", "\"130\"", "not json"] {
        let res = app.clone().oneshot(put(bad)).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{bad}");
    }
    assert_eq!(
        get(app.clone()).await,
        serde_json::json!(130),
        "bad bodies change nothing"
    );
    let unauthorized = get_req("/api/admin/text-size", "127.0.0.1:47810")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.oneshot(unauthorized).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn tab_bar_persists_and_rejects_bad_bodies() {
    let dir = std::env::temp_dir().join(format!("st-host-tabbar-{}", std::process::id()));
    let app_for = |dir: &PathBuf| {
        let mut config = test_config();
        config.data_dir = dir.clone();
        router(config).unwrap()
    };
    let put = |body: &str| {
        get_req("/api/admin/tab-bar", "127.0.0.1:47810")
            .method(Method::PUT)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap()
    };
    let get = |app: Router| async move {
        let res = app
            .oneshot(
                get_req("/api/admin/tab-bar", "127.0.0.1:47810")
                    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()
    };

    let app = app_for(&dir);
    assert_eq!(get(app.clone()).await, serde_json::Value::Null);
    let res = app
        .clone()
        .oneshot(put(
            r#"{"groups":["media","home","media"],"position":"bottom"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    // A new router on the same data folder stands in for a restart.
    assert_eq!(
        get(app_for(&dir)).await,
        serde_json::json!({ "groups": ["media", "home"], "position": "bottom" })
    );
    let res = app
        .clone()
        .oneshot(put(r#"{"groups":[],"position":"top"}"#))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        get(app.clone()).await,
        serde_json::json!({ "groups": [], "position": "top" }),
        "an empty list pins nothing, unlike never edited"
    );

    let too_many = format!(
        r#"{{"groups":{},"position":"top"}}"#,
        serde_json::to_string(&vec!["a"; 21]).unwrap()
    );
    for bad in [
        r#"{"groups":["Home"],"position":"top"}"#,
        r#"{"groups":["home"],"position":"left"}"#,
        r#"{"groups":["home"]}"#,
        "not json",
        too_many.as_str(),
    ] {
        let res = app.clone().oneshot(put(bad)).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{bad}");
    }
    let res = app.clone().oneshot(put("null")).await.unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(get(app.clone()).await, serde_json::Value::Null);
    let saved = std::fs::read_to_string(settings::path(&dir)).unwrap();
    assert!(!saved.contains("tab_bar"), "{saved}");

    let unauthorized = get_req("/api/admin/tab-bar", "127.0.0.1:47810")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.oneshot(unauthorized).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn chat_dock_persists_and_rejects_bad_bodies() {
    let dir = std::env::temp_dir().join(format!("st-host-dock-{}", std::process::id()));
    let app_for = |dir: &PathBuf| {
        let mut config = test_config();
        config.data_dir = dir.clone();
        router(config).unwrap()
    };
    let put = |body: &str| {
        get_req("/api/admin/chat-dock", "127.0.0.1:47810")
            .method(Method::PUT)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap()
    };
    let get = |app: Router| async move {
        let res = app
            .oneshot(
                get_req("/api/admin/chat-dock", "127.0.0.1:47810")
                    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()
    };

    let app = app_for(&dir);
    assert_eq!(get(app.clone()).await, serde_json::Value::Null);
    let res = app
        .clone()
        .oneshot(put(r#"{"open":true,"width":360}"#))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    // A new router on the same data folder stands in for a restart.
    assert_eq!(
        get(app_for(&dir)).await,
        serde_json::json!({ "open": true, "width": 360 })
    );

    for bad in [
        r#"{"open":true,"width":279}"#,
        r#"{"open":true,"width":561}"#,
        r#"{"open":true}"#,
        "not json",
    ] {
        let res = app.clone().oneshot(put(bad)).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{bad}");
    }
    assert_eq!(
        get(app.clone()).await,
        serde_json::json!({ "open": true, "width": 360 }),
        "bad bodies change nothing"
    );
    let unauthorized = get_req("/api/admin/chat-dock", "127.0.0.1:47810")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.oneshot(unauthorized).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let _ = std::fs::remove_dir_all(dir);
}
