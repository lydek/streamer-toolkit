use super::*;

/// Every documented admin route, so one added later is covered without touching this test.
#[tokio::test]
async fn every_admin_route_in_the_openapi_document_requires_the_token() {
    let doc = openapi::openapi();
    let mut checked = Vec::new();
    for (path, item) in doc.paths.paths.iter() {
        if !path.starts_with("/api/admin/") {
            continue;
        }
        let uri = path
            .split('/')
            .map(|s| if s.starts_with('{') { "x" } else { s })
            .collect::<Vec<_>>()
            .join("/");
        for (method, documented) in [
            (Method::GET, item.get.is_some()),
            (Method::PUT, item.put.is_some()),
            (Method::POST, item.post.is_some()),
            (Method::DELETE, item.delete.is_some()),
            (Method::PATCH, item.patch.is_some()),
        ] {
            if !documented {
                continue;
            }
            let req = get_req(&uri, "127.0.0.1:47810")
                .method(method.clone())
                .header(header::ORIGIN, "http://127.0.0.1:47810")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap();
            assert_eq!(
                status(req).await,
                StatusCode::UNAUTHORIZED,
                "{method} {path}"
            );
            checked.push(format!("{method} {path}"));
        }
    }
    // The document really lists the admin API (the twitch, backup, sound and check-in groups at least).
    for route in [
        "GET /api/admin/twitch",
        "POST /api/admin/backup/restore",
        "DELETE /api/admin/sounds/{id}",
        "GET /api/admin/checkin/viewers",
    ] {
        assert!(
            checked.iter().any(|c| c == route),
            "{route} not checked: {checked:?}"
        );
    }
}

#[tokio::test]
async fn admin_api_requires_the_token() {
    let no_token = get_req("/api/admin/twitch", "127.0.0.1:47810")
        .body(Body::empty())
        .unwrap();
    assert_eq!(status(no_token).await, StatusCode::UNAUTHORIZED);

    let wrong = get_req("/api/admin/twitch", "127.0.0.1:47810")
        .header(header::AUTHORIZATION, "Bearer nope")
        .body(Body::empty())
        .unwrap();
    assert_eq!(status(wrong).await, StatusCode::UNAUTHORIZED);

    let ok = get_req("/api/admin/twitch", "127.0.0.1:47810")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let res = app().oneshot(ok).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
    let overview: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        overview["eventsub_connected"], false,
        "signed out: no EventSub session"
    );
    assert_eq!(
        overview["irc_connected"], false,
        "signed out: first chats fall back to the toolkit's first sighting"
    );
}

#[tokio::test]
async fn port_zero_binds_a_free_port_and_allows_it() {
    let mut config = test_config();
    config.port = 0;
    let bound = bind(config).await.unwrap();
    let port = bound.port();
    assert_ne!(port, 0);
    let res = bound
        .router
        .oneshot(
            get_req("/api/health", &format!("127.0.0.1:{port}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "the Host allowlist uses the bound port"
    );
}

fn ws_req(origin: Option<&str>) -> Request {
    let mut b = get_req("/ws/overlay", "127.0.0.1:47810")
        .header(header::UPGRADE, "websocket")
        .header(header::CONNECTION, "upgrade")
        .header(header::SEC_WEBSOCKET_VERSION, "13")
        .header(header::SEC_WEBSOCKET_KEY, "dGhlIHNhbXBsZSBub25jZQ==");
    if let Some(o) = origin {
        b = b.header(header::ORIGIN, o);
    }
    b.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn rejects_foreign_host_header() {
    let req = get_req("/api/health", "evil.example:47810")
        .body(Body::empty())
        .unwrap();
    assert_eq!(status(req).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn accepts_loopback_host_header() {
    for host in ["127.0.0.1:47810", "localhost:47810"] {
        let req = get_req("/api/health", host).body(Body::empty()).unwrap();
        assert_eq!(status(req).await, StatusCode::OK, "{host}");
    }
}

#[tokio::test]
async fn websocket_requires_allowed_origin() {
    assert_eq!(status(ws_req(None)).await, StatusCode::FORBIDDEN);
    assert_eq!(status(ws_req(Some("null"))).await, StatusCode::FORBIDDEN);
    assert_eq!(
        status(ws_req(Some("https://evil.example"))).await,
        StatusCode::FORBIDDEN
    );
    assert_ne!(
        status(ws_req(Some("http://127.0.0.1:47810"))).await,
        StatusCode::FORBIDDEN
    );
    assert_ne!(
        status(ws_req(Some("http://localhost:5173"))).await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn state_changing_request_requires_origin() {
    let req = get_req("/api/health", "127.0.0.1:47810")
        .method(Method::POST)
        .body(Body::empty())
        .unwrap();
    assert_eq!(status(req).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn admin_gets_csp_and_spa_fallback() {
    for uri in ["/admin", "/admin/settings"] {
        let res = app()
            .oneshot(get_req(uri, "127.0.0.1:47810").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        // Same-origin pages could reach the admin IPC capability if framed: only the preview page may be.
        assert_eq!(
            res.headers()[header::CONTENT_SECURITY_POLICY],
            format!(
                "{ADMIN_CSP}; frame-src http://127.0.0.1:47810/overlay/chat/ \
                 http://127.0.0.1:47810/overlay/alerts/ http://localhost:47810/overlay/chat/ \
                 http://localhost:47810/overlay/alerts/"
            )
        );
    }
    assert!(!ADMIN_CSP.contains("frame-src"));
}

#[tokio::test]
async fn overlay_gets_overlay_csp_and_assets_get_cors() {
    for page in [
        "/overlay/chat",
        "/overlay/alerts",
        "/overlay/checkin",
        "/overlay/sound",
        "/overlay/bgm",
        "/overlay/counter",
        "/overlay/marathon",
    ] {
        let res = app()
            .oneshot(
                get_req(page, "127.0.0.1:47810")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK, "{page}");
        assert_eq!(res.headers()[header::CONTENT_SECURITY_POLICY], OVERLAY_CSP);
    }

    let asset = Web::iter()
        .find(|p| p.starts_with("assets/"))
        .expect("built assets");
    let res = app()
        .oneshot(
            get_req(&format!("/{asset}"), "127.0.0.1:47810")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
}
