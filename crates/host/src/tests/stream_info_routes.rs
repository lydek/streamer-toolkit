//! 直播資訊 against a fake Helix, signed in from a stored token.
use super::*;
use axum::{
    Json,
    extract::{Query, State as AxumState},
    routing::get,
};
use std::{collections::HashMap, sync::Mutex};

/// A stored sign-in for client ID `cid`, user 42.
struct SignedIn;
impl SecretStore for SignedIn {
    fn read(&self) -> Result<Option<String>, SecretStoreError> {
        Ok(Some(
            serde_json::json!({
                "client_id": "cid", "access_token": "a", "refresh_token": "r",
                "user_id": "42", "login": "me", "scopes": ["channel:manage:broadcast"]
            })
            .to_string(),
        ))
    }
    fn write(&self, _: &str) -> Result<(), SecretStoreError> {
        Ok(())
    }
    fn delete(&self) -> Result<(), SecretStoreError> {
        Ok(())
    }
}

#[derive(Clone, Default)]
struct Fake {
    patches: Arc<Mutex<Vec<serde_json::Value>>>,
    searches: Arc<Mutex<Vec<String>>>,
    /// Status the PATCH answers with.
    patch_status: Arc<Mutex<u16>>,
}

async fn fake_helix(fake: Fake) -> String {
    let app = Router::new()
        .route(
            "/channels",
            get(|| async {
                Json(serde_json::json!({ "data": [{
                    "broadcaster_id": "42", "title": "唱歌", "game_id": "26936", "game_name": "Music",
                    "tags": ["中文"], "broadcaster_language": "zh"
                }] }))
            })
            .patch(|AxumState(f): AxumState<Fake>, Json(body): Json<serde_json::Value>| async move {
                f.patches.lock().unwrap().push(body);
                StatusCode::from_u16(*f.patch_status.lock().unwrap()).unwrap()
            }),
        )
        .route(
            "/search/categories",
            get(|AxumState(f): AxumState<Fake>, Query(q): Query<HashMap<String, String>>| async move {
                f.searches.lock().unwrap().push(q["query"].clone());
                Json(serde_json::json!({ "data": [{ "id": "26936", "name": "Music", "box_art_url": "" }] }))
            }),
        )
        .with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

async fn signed_in_app(name: &str, fake: &Fake) -> Router {
    let dir =
        std::env::temp_dir().join(format!("st-host-stream-info-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    settings::update(&dir, |s| s.twitch_client_id = "cid".into()).unwrap();
    let mut config = test_config();
    config.data_dir = dir;
    config.twitch_secrets = Arc::new(SignedIn);
    config.twitch.helix = fake_helix(fake.clone()).await;
    router(config).unwrap()
}

fn req(method: Method, uri: &str, body: Option<serde_json::Value>) -> Request {
    let b = get_req(uri, "127.0.0.1:47810")
        .method(method)
        .header(header::ORIGIN, "http://127.0.0.1:47810")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"));
    match body {
        Some(json) => b
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json.to_string())),
        None => b.body(Body::empty()),
    }
    .unwrap()
}

async fn json(res: axum::response::Response) -> serde_json::Value {
    let body = axum::body::to_bytes(res.into_body(), 65536).await.unwrap();
    serde_json::from_slice(&body).unwrap_or_default()
}

#[tokio::test]
async fn reads_the_channel_and_writes_what_was_changed() {
    let fake = Fake {
        patch_status: Arc::new(Mutex::new(204)),
        ..Fake::default()
    };
    let app = signed_in_app("rw", &fake).await;

    let got = app
        .clone()
        .oneshot(req(Method::GET, "/api/admin/stream-info", None))
        .await
        .unwrap();
    assert_eq!(got.status(), StatusCode::OK);
    assert_eq!(
        json(got).await,
        serde_json::json!({ "title": "唱歌", "category": { "id": "26936", "name": "Music" }, "tags": ["中文"], "language": "zh" })
    );

    let body = serde_json::json!({ "title": "週二點歌夜", "category_id": "26936", "tags": ["中文", "唱歌"], "language": "zh" });
    let res = app
        .clone()
        .oneshot(req(Method::PUT, "/api/admin/stream-info", Some(body)))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        fake.patches.lock().unwrap().as_slice(),
        [
            serde_json::json!({ "title": "週二點歌夜", "game_id": "26936", "tags": ["中文", "唱歌"], "broadcaster_language": "zh" })
        ]
    );
}

#[tokio::test]
async fn over_twitch_limits_is_refused_here_and_a_missing_scope_says_so() {
    let fake = Fake {
        patch_status: Arc::new(Mutex::new(403)),
        ..Fake::default()
    };
    let app = signed_in_app("limits", &fake).await;
    for body in [
        serde_json::json!({ "title": "x".repeat(141), "category_id": "", "tags": [], "language": "zh" }),
        serde_json::json!({ "title": "ok", "category_id": "", "tags": vec!["t"; 11], "language": "zh" }),
        serde_json::json!({ "title": "ok", "category_id": "abc", "tags": [], "language": "zh" }),
    ] {
        let res = app
            .clone()
            .oneshot(req(Method::PUT, "/api/admin/stream-info", Some(body)))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json(res).await["code"], "stream_info.invalid");
    }
    assert!(fake.patches.lock().unwrap().is_empty());

    let body =
        serde_json::json!({ "title": "ok", "category_id": "", "tags": [], "language": "zh" });
    let res = app
        .oneshot(req(Method::PUT, "/api/admin/stream-info", Some(body)))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(json(res).await["code"], "twitch.scope_required");
}

#[tokio::test]
async fn categories_are_searched_by_name() {
    let fake = Fake::default();
    let app = signed_in_app("search", &fake).await;
    let res = app
        .oneshot(req(
            Method::GET,
            "/api/admin/stream-info/categories?query=Just%20Chatting",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(
        json(res).await,
        serde_json::json!([{ "id": "26936", "name": "Music" }])
    );
    assert_eq!(fake.searches.lock().unwrap().as_slice(), ["Just Chatting"]);
}
