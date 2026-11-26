//! Writing to Twitch (直播資訊, sending chat) against a fake Helix, signed in from a stored token.
use super::*;
use axum::{
    Json,
    extract::{Query, State as AxumState},
    routing::get,
};
use std::{collections::HashMap, sync::Mutex};

/// A stored sign-in for client ID `cid`, user 42, with these scopes.
struct SignedIn(&'static [&'static str]);
impl SecretStore for SignedIn {
    fn read(&self) -> Result<Option<String>, SecretStoreError> {
        Ok(Some(
            serde_json::json!({
                "client_id": "cid", "access_token": "a", "refresh_token": "r",
                "user_id": "42", "login": "me", "scopes": self.0
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
    sent: Arc<Mutex<Vec<serde_json::Value>>>,
    /// Whether Twitch says a sent message went out.
    is_sent: Arc<Mutex<bool>>,
    /// Status a sent message is refused with; 0: accepted.
    send_refused: Arc<Mutex<u16>>,
    /// Reward requests: method, query, body.
    rewards: Arc<Mutex<Vec<(String, String, serde_json::Value)>>>,
    /// Status reward changes answer with; 0: done.
    reward_status: Arc<Mutex<u16>>,
    /// More rewards on the channel besides the two (made in the dashboard).
    extra_rewards: Arc<Mutex<usize>>,
}

/// A Helix custom reward; `r-own` is the toolkit's, `r-dash` was made in the Twitch dashboard.
fn helix_reward(id: &str, title: &str) -> serde_json::Value {
    serde_json::json!({
        "broadcaster_id": "42", "id": id, "title": title, "prompt": "", "cost": 100,
        "background_color": "#9146FF", "is_enabled": true, "is_paused": false,
        "is_user_input_required": false, "should_redemptions_skip_request_queue": false,
        "global_cooldown_setting": { "is_enabled": true, "global_cooldown_seconds": 300 },
        "max_per_stream_setting": { "is_enabled": false, "max_per_stream": 0 },
        "max_per_user_per_stream_setting": { "is_enabled": true, "max_per_user_per_stream": 3 },
        "image": null, "default_image": {}
    })
}

async fn reward_request(
    f: Fake,
    method: Method,
    query: Option<String>,
    body: Option<serde_json::Value>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let query = query.unwrap_or_default();
    if method == Method::GET {
        let mut data = vec![helix_reward("r-own", "每日打卡")];
        if !query.contains("only_manageable_rewards=true") {
            data.push(helix_reward("r-dash", "Hydrate!"));
            for n in 0..*f.extra_rewards.lock().unwrap() {
                data.push(helix_reward(&format!("r-{n}"), &format!("extra {n}")));
            }
        }
        return Json(serde_json::json!({ "data": data })).into_response();
    }
    let body = body.unwrap_or_default();
    let made = (method == Method::POST)
        .then(|| helix_reward("r-new", body["title"].as_str().unwrap_or_default()));
    f.rewards
        .lock()
        .unwrap()
        .push((method.to_string(), query, body));
    match *f.reward_status.lock().unwrap() {
        0 => Json(serde_json::json!({ "data": [made.unwrap_or_else(|| helix_reward("r-own", "每日打卡"))] }))
            .into_response(),
        status => StatusCode::from_u16(status).unwrap().into_response(),
    }
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
        .route(
            "/chat/messages",
            axum::routing::post(|AxumState(f): AxumState<Fake>, Json(body): Json<serde_json::Value>| async move {
                use axum::response::IntoResponse;
                f.sent.lock().unwrap().push(body);
                let refused = *f.send_refused.lock().unwrap();
                if refused != 0 {
                    return StatusCode::from_u16(refused).unwrap().into_response();
                }
                let is_sent = *f.is_sent.lock().unwrap();
                Json(serde_json::json!({ "data": [{ "message_id": "m1", "is_sent": is_sent, "drop_reason": null }] }))
                    .into_response()
            }),
        )
        .route(
            "/channel_points/custom_rewards",
            axum::routing::any(
                |AxumState(f): AxumState<Fake>,
                 method: Method,
                 axum::extract::RawQuery(q): axum::extract::RawQuery,
                 body: Option<Json<serde_json::Value>>| async move {
                    reward_request(f, method, q, body.map(|Json(b)| b)).await
                },
            ),
        )
        .with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

async fn signed_in_app(name: &str, fake: &Fake) -> Router {
    signed_in_with(
        name,
        fake,
        &["channel:manage:broadcast", "channel:manage:redemptions"],
    )
    .await
}

async fn signed_in_with(name: &str, fake: &Fake, scopes: &'static [&'static str]) -> Router {
    let dir =
        std::env::temp_dir().join(format!("st-host-stream-info-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    settings::update(&dir, |s| s.twitch_client_id = "cid".into()).unwrap();
    let mut config = test_config();
    config.data_dir = dir;
    config.twitch_secrets = Arc::new(SignedIn(scopes));
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

#[tokio::test]
async fn chat_is_sent_as_the_streamer_and_a_dropped_message_says_so() {
    let fake = Fake {
        is_sent: Arc::new(Mutex::new(true)),
        ..Fake::default()
    };
    let app = signed_in_app("chat", &fake).await;
    let send = |text: &str| {
        req(
            Method::POST,
            "/api/admin/chat/send",
            Some(serde_json::json!({ "text": text })),
        )
    };

    let res = app.clone().oneshot(send("  大家好  ")).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json(res).await, serde_json::json!({ "message_id": "m1" }));
    assert_eq!(
        fake.sent.lock().unwrap().as_slice(),
        [serde_json::json!({ "broadcaster_id": "42", "sender_id": "42", "message": "大家好" })]
    );

    for text in ["   ", &"字".repeat(501)] {
        let res = app.clone().oneshot(send(text)).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json(res).await["code"], "chat.send_invalid");
    }
    assert_eq!(fake.sent.lock().unwrap().len(), 1);

    *fake.is_sent.lock().unwrap() = false;
    let res = app.clone().oneshot(send("again")).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json(res).await["code"], "chat.send_dropped");

    // Twitch refuses the message itself: said as such, not as a Twitch outage.
    *fake.send_refused.lock().unwrap() = 400;
    let res = app.oneshot(send("again")).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json(res).await["code"], "chat.send_refused");
}

#[tokio::test]
async fn rewards_are_listed_with_the_ones_the_toolkit_made_marked() {
    let fake = Fake::default();
    let app = signed_in_app("rewards-list", &fake).await;
    let res = app
        .oneshot(req(Method::GET, "/api/admin/rewards", None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let list = json(res).await;
    assert_eq!(list[0]["id"], "r-own");
    assert_eq!(list[0]["manageable"], true);
    assert_eq!(list[0]["cooldown_seconds"], 300);
    assert_eq!(list[0]["max_per_stream"], serde_json::Value::Null);
    assert_eq!(list[0]["max_per_user_per_stream"], 3);
    assert_eq!(list[1]["id"], "r-dash");
    assert_eq!(list[1]["manageable"], false);
}

#[tokio::test]
async fn a_reward_is_enabled_or_paused_only_with_the_scope_and_when_it_is_ours() {
    let fake = Fake::default();
    let app = signed_in_app("rewards-toggle", &fake).await;
    let patch = |id: &str, body: serde_json::Value| {
        req(
            Method::PATCH,
            &format!("/api/admin/rewards/{id}"),
            Some(body),
        )
    };

    let res = app
        .clone()
        .oneshot(patch("r-own", serde_json::json!({ "is_paused": true })))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        fake.rewards.lock().unwrap().as_slice(),
        [(
            "PATCH".to_owned(),
            "broadcaster_id=42&id=r-own".to_owned(),
            serde_json::json!({ "is_paused": true })
        )]
    );

    // Nothing to change, or an ID that could break the URL: refused here.
    for (id, body) in [
        ("r-own", serde_json::json!({})),
        ("r%26id%3Dx", serde_json::json!({ "is_enabled": false })),
    ] {
        let res = app.clone().oneshot(patch(id, body)).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json(res).await["code"], "rewards.invalid");
    }
    assert_eq!(fake.rewards.lock().unwrap().len(), 1);

    // Twitch refuses a reward made in its dashboard.
    *fake.reward_status.lock().unwrap() = 403;
    let res = app
        .oneshot(patch("r-dash", serde_json::json!({ "is_enabled": false })))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(json(res).await["code"], "rewards.not_ours");

    // Without the scope nothing is sent.
    let fake = Fake::default();
    let app = signed_in_with("rewards-no-scope", &fake, &["channel:read:redemptions"]).await;
    let res = app
        .oneshot(patch("r-own", serde_json::json!({ "is_enabled": false })))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(json(res).await["code"], "twitch.scope_required");
    assert!(fake.rewards.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_reward_is_added_unless_the_title_is_taken_or_the_channel_is_full() {
    let fake = Fake::default();
    let app = signed_in_app("rewards-create", &fake).await;
    let create = |body: serde_json::Value| req(Method::POST, "/api/admin/rewards", Some(body));

    let res = app
        .clone()
        .oneshot(create(
            serde_json::json!({ "title": "喝水提醒", "cost": 200, "cooldown_seconds": 600 }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json(res).await, serde_json::json!({ "id": "r-new" }));
    assert_eq!(
        fake.rewards.lock().unwrap().as_slice(),
        [(
            "POST".to_owned(),
            "broadcaster_id=42".to_owned(),
            serde_json::json!({
                "title": "喝水提醒", "cost": 200,
                "is_global_cooldown_enabled": true, "global_cooldown_seconds": 600
            })
        )]
    );

    // Twitch ignores case when it refuses a second reward with the same title: told here, nothing sent.
    let res = app
        .clone()
        .oneshot(create(
            serde_json::json!({ "title": "HYDRATE!", "cost": 1 }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(json(res).await["code"], "rewards.duplicate_title");
    // No cost: not a reward.
    let res = app
        .clone()
        .oneshot(create(serde_json::json!({ "title": "x" })))
        .await
        .unwrap();
    assert_eq!(json(res).await["code"], "rewards.invalid");
    // 50 rewards already.
    *fake.extra_rewards.lock().unwrap() = 48;
    let res = app
        .oneshot(create(serde_json::json!({ "title": "新的", "cost": 1 })))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(json(res).await["code"], "rewards.limit");
    assert_eq!(fake.rewards.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_reward_is_edited_or_deleted_on_twitch() {
    let fake = Fake::default();
    let app = signed_in_app("rewards-edit", &fake).await;
    let edit = |body: serde_json::Value| req(Method::PATCH, "/api/admin/rewards/r-own", Some(body));

    // Its own title again is fine; another reward's is not.
    let res = app
        .clone()
        .oneshot(edit(
            serde_json::json!({ "title": "每日打卡", "max_per_stream": 0 }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = app
        .clone()
        .oneshot(edit(serde_json::json!({ "title": "hydrate!" })))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(json(res).await["code"], "rewards.duplicate_title");

    let res = app
        .oneshot(req(Method::DELETE, "/api/admin/rewards/r-own", None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let sent: Vec<_> = fake
        .rewards
        .lock()
        .unwrap()
        .iter()
        .map(|(m, q, b)| (m.clone(), q.clone(), b.clone()))
        .collect();
    assert_eq!(
        sent,
        [
            (
                "PATCH".to_owned(),
                "broadcaster_id=42&id=r-own".to_owned(),
                serde_json::json!({ "title": "每日打卡", "is_max_per_stream_enabled": false })
            ),
            (
                "DELETE".to_owned(),
                "broadcaster_id=42&id=r-own".to_owned(),
                serde_json::Value::Null
            ),
        ]
    );
}
