use super::*;

/// A Host with its own data folder, so template tests can change files without racing each other.
fn template_app(name: &str) -> (Router, PathBuf) {
    let dir = std::env::temp_dir().join(format!("st-tpl-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    (template_router(&dir), dir)
}

fn template_router(dir: &std::path::Path) -> Router {
    let mut config = test_config();
    config.data_dir = dir.to_path_buf();
    router(config).unwrap()
}

/// `custom` selected, with a copy of the default template in the data folder.
fn custom_template_app(name: &str) -> (Router, PathBuf) {
    let dir = std::env::temp_dir().join(format!("st-tpl-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    settings::update(&dir, |s| s.chat_template = "custom".into()).unwrap();
    templates::tests::write_default(&dir.join("templates/chat"));
    (template_router(&dir), dir)
}

#[tokio::test]
async fn alert_template_is_served_like_the_chat_ones() {
    let app = app();
    let (status, headers, html) = get_body(&app, "/templates/alerts/template.html").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_SECURITY_POLICY], OVERLAY_CSP);
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    // The markers apps/web/src/overlay/alerts/template.ts binds to.
    assert!(html.contains("data-alert-stage") && html.contains("<template data-alert>"));
    let (status, headers, _) = get_body(&app, "/templates/alerts/style.css").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "text/css; charset=utf-8");
    for (uri, expected) in [
        ("/templates/alerts/evil.js", StatusCode::NOT_FOUND),
        (
            "/templates/alerts/../chat/template.html",
            StatusCode::BAD_REQUEST,
        ),
        ("/templates/alerts/%2E%2E/x.css", StatusCode::BAD_REQUEST),
    ] {
        assert_eq!(get_body(&app, uri).await.0, expected, "{uri}");
    }
}

fn put_template(id: &str) -> Request {
    get_req("/api/admin/templates/chat", "127.0.0.1:47810")
        .method(Method::PUT)
        .header(header::ORIGIN, "http://127.0.0.1:47810")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(format!(r#"{{"selected":"{id}"}}"#)))
        .unwrap()
}

async fn get_body(app: &Router, uri: &str) -> (StatusCode, axum::http::HeaderMap, String) {
    let res = app
        .clone()
        .oneshot(get_req(uri, "127.0.0.1:47810").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (parts, body) = res.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    (
        parts.status,
        parts.headers,
        String::from_utf8_lossy(&bytes).into(),
    )
}

async fn template_status(app: &Router) -> serde_json::Value {
    let res = app
        .clone()
        .oneshot(
            get_req("/api/admin/templates/chat", "127.0.0.1:47810")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
}

async fn template_error(app: &Router) -> serde_json::Value {
    template_status(app).await["error"].clone()
}

/// The watcher polls every second; give it a few rounds.
async fn wait_for_error(app: &Router, code: &str) {
    for _ in 0..50 {
        if template_error(app).await == code {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("template error never became {code}");
}

#[tokio::test]
async fn first_run_serves_the_built_in_default_with_overlay_headers() {
    let (app, dir) = template_app("default");
    assert!(
        !dir.join("templates/chat").exists(),
        "the user folder is not created"
    );
    assert_eq!(
        template_status(&app).await,
        serde_json::json!({ "selected": "default", "error": null })
    );
    let (status, headers, body) = get_body(&app, "/templates/chat/template.html").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("data-chat-list"));
    assert_eq!(headers[header::CONTENT_TYPE], "text/html; charset=utf-8");
    assert_eq!(headers[header::CONTENT_SECURITY_POLICY], OVERLAY_CSP);
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
    assert_eq!(template_error(&app).await, serde_json::Value::Null);
    let no_token = get_req("/api/admin/templates/chat", "127.0.0.1:47810")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(no_token).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    // Error answers carry the same headers, also the ones produced before the handler runs.
    for uri in [
        "/templates/chat/nope.css",
        "/templates/chat/%ff.css",
        "/templates/chat/",
        "/templates",
        "/templates/built-in/chat/nope/template.html",
    ] {
        let (status, headers, _) = get_body(&app, uri).await;
        assert!(!status.is_success(), "{uri}");
        assert_eq!(
            headers[header::CONTENT_SECURITY_POLICY],
            OVERLAY_CSP,
            "{uri}"
        );
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff", "{uri}");
        assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*", "{uri}");
    }
    let foreign_host = app
        .clone()
        .oneshot(
            get_req("/templates/chat/style.css", "evil.example:47810")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(foreign_host.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        foreign_host.headers()[header::CONTENT_SECURITY_POLICY],
        OVERLAY_CSP
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn switching_templates_applies_and_survives_a_restart() {
    let (app, dir) = template_app("switch");
    let mut no_token = put_template("bubble");
    no_token.headers_mut().remove(header::AUTHORIZATION);
    assert_eq!(
        app.clone().oneshot(no_token).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.clone()
            .oneshot(put_template("bubble"))
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(template_status(&app).await["selected"], "bubble");
    let (_, _, body) = get_body(&app, "/templates/chat/template.html").await;
    assert!(body.contains(r#"class="chat bubble""#), "{body}");
    for bad in ["nope", "Bubble", ""] {
        let res = app.clone().oneshot(put_template(bad)).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{bad}");
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["code"],
            "template.unknown"
        );
    }
    assert_eq!(settings::load(&dir).chat_template, "bubble");
    let restarted = template_router(&dir);
    assert_eq!(template_status(&restarted).await["selected"], "bubble");
    // A hand-edited unknown value means the default.
    settings::update(&dir, |s| s.chat_template = "gone".into()).unwrap();
    assert_eq!(
        template_status(&template_router(&dir)).await["selected"],
        "default"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn every_built_in_template_is_served_for_previews() {
    let (app, dir) = template_app("built-in");
    for id in ["default", "compact", "bubble"] {
        let (status, headers, body) = get_body(
            &app,
            &format!("/templates/built-in/chat/{id}/template.html"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{id}");
        assert!(body.contains("data-chat-list"), "{id}");
        assert_eq!(headers[header::CONTENT_SECURITY_POLICY], OVERLAY_CSP);
        assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        let (status, _, _) =
            get_body(&app, &format!("/templates/built-in/chat/{id}/style.css")).await;
        assert_eq!(status, StatusCode::OK, "{id}");
    }
    for uri in [
        "/templates/built-in/chat/custom/template.html",
        "/templates/built-in/chat/default/evil.js",
        "/templates/built-in/chat/default/../bubble/style.css",
        "/templates/built-in/chat/default/..%2fbubble%2fstyle.css",
    ] {
        let (status, _, _) = get_body(&app, uri).await;
        assert!(!status.is_success(), "{uri} -> {status}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn custom_template_serves_assets_in_subfolders() {
    let (app, dir) = custom_template_app("assets");
    assert_eq!(
        template_status(&app).await,
        serde_json::json!({ "selected": "custom", "error": null })
    );
    std::fs::create_dir_all(dir.join("templates/chat/img")).unwrap();
    std::fs::write(dir.join("templates/chat/img/logo.svg"), "<svg/>").unwrap();
    let (status, headers, body) = get_body(&app, "/templates/chat/img/logo.svg").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "image/svg+xml");
    assert_eq!(body, "<svg/>");
    // The user folder is not used while a built-in template is selected.
    app.clone().oneshot(put_template("compact")).await.unwrap();
    let (status, _, _) = get_body(&app, "/templates/chat/img/logo.svg").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn template_route_never_serves_scripts() {
    let (app, dir) = custom_template_app("scripts");
    let chat = dir.join("templates/chat");
    for name in ["evil.js", "evil.mjs", "evil.JS", "data.json"] {
        std::fs::write(chat.join(name), "alert(1)").unwrap();
        let (status, _, body) = get_body(&app, &format!("/templates/chat/{name}")).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{name}");
        assert!(!body.contains("alert"), "{name}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn template_route_rejects_path_escapes() {
    let (app, dir) = custom_template_app("escape");
    std::fs::write(dir.join("secret.css"), "secret").unwrap();
    std::fs::write(dir.join("templates/secret.css"), "secret").unwrap();
    for uri in [
        "/templates/chat/../secret.css",
        "/templates/chat/../../secret.css",
        "/templates/chat/%2e%2e/%2e%2e/secret.css",
        "/templates/chat/..%2f..%2fsecret.css",
        "/templates/chat/..%5c..%5csecret.css",
        "/templates/chat/sub/..%5c..%5c..%5csecret.css",
        "/templates/chat/C:%5cWindows%5cwin.ini.css",
        "/templates/chat//secret.css",
    ] {
        let (status, _, body) = get_body(&app, uri).await;
        assert!(!status.is_success(), "{uri} -> {status}");
        assert!(!body.contains("secret"), "{uri}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn missing_or_broken_template_falls_back_to_the_default() {
    let (app, dir) = custom_template_app("fallback");
    let chat = dir.join("templates/chat");
    std::fs::write(chat.join("template.html"), "<p>broken</p>").unwrap();
    std::fs::write(chat.join("style.css"), "user-css").unwrap();
    wait_for_error(&app, "template.invalid").await;
    let (status, _, body) = get_body(&app, "/templates/chat/template.html").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("data-chat-list"), "default HTML, never blank");
    let (_, _, css) = get_body(&app, "/templates/chat/style.css").await;
    assert!(!css.contains("user-css"), "the default comes as a whole");

    std::fs::remove_file(chat.join("template.html")).unwrap();
    wait_for_error(&app, "template.missing").await;
    let (status, _, body) = get_body(&app, "/templates/chat/template.html").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("data-chat-list"));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn custom_template_change_is_pushed_to_overlays() {
    let dir = std::env::temp_dir().join(format!("st-tpl-watch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    settings::update(&dir, |s| s.chat_template = "custom".into()).unwrap();
    templates::tests::write_default(&dir.join("templates/chat"));
    let bus = Arc::new(toolkit_core::bus::EventBus::default());
    let _templates = templates::Templates::start(&dir, bus.clone());
    let (_, mut events) = bus.subscribe();
    std::fs::write(dir.join("templates/chat/style.css"), ".chat{color:red}").unwrap();
    let event = tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
        .await
        .expect("notified within a couple of seconds")
        .unwrap();
    let event: serde_json::Value = serde_json::from_str(&event).unwrap();
    assert_eq!(event["type"], "template.changed");
    assert_eq!(event["source"], "system");
    assert_eq!(event["payload"]["overlay"], "chat");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn user_folder_changes_are_ignored_while_a_built_in_template_is_selected() {
    let dir = std::env::temp_dir().join(format!("st-tpl-quiet-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    templates::tests::write_default(&dir.join("templates/chat"));
    let bus = Arc::new(toolkit_core::bus::EventBus::default());
    let _templates = templates::Templates::start(&dir, bus.clone());
    let (_, mut events) = bus.subscribe();
    std::fs::write(dir.join("templates/chat/style.css"), ".chat{color:red}").unwrap();
    // Two and a half polls.
    let event = tokio::time::timeout(std::time::Duration::from_millis(2500), events.recv()).await;
    assert!(event.is_err(), "no reload for an unused folder: {event:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_old_chat_text_size_carries_over_into_the_overlay_text_size_once() {
    let dir = std::env::temp_dir().join(format!("st-tpl-carry-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // 36px with the default template (24px) is 150%.
    settings::update(&dir, |s| s.chat_text_size = Some(36)).unwrap();
    let mut config = test_config();
    config.data_dir = dir.clone();
    config.db = db::memory_migrated().await;
    let app = router(config).unwrap();
    for _ in 0..200 {
        if settings::load(&dir).chat_text_size.is_none() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(
        settings::load(&dir).chat_text_size,
        None,
        "cleared, so it runs once"
    );
    let res = app
        .clone()
        .oneshot(
            get_req("/api/admin/screen-tools/appearance", "127.0.0.1:47810")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let appearance: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(appearance["textSize"], 150);
    let _ = std::fs::remove_dir_all(dir);
}
