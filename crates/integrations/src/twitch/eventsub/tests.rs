use super::*;
use crate::{
    secrets::memory::MemoryStore,
    tls::{self, HttpsClient},
    twitch::auth::TwitchAuth,
};
use axum::{
    Json, Router,
    extract::{State, WebSocketUpgrade, ws},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, post},
};
use serde_json::{Value, json};
use std::sync::Mutex;
use tokio::sync::mpsc;
use toolkit_core::events::PollStatus;

/// Client-Id header and JSON body of one subscription request.
type Subscription = (Option<String>, Value);

#[derive(Clone, Default)]
struct Fake {
    base: Arc<Mutex<String>>,
    subscriptions: Arc<Mutex<Vec<Subscription>>>,
}

fn welcome(id: &str) -> String {
    welcome_with(id, 10)
}

fn welcome_with(id: &str, keepalive_secs: u64) -> String {
    json!({"metadata":{"message_type":"session_welcome","message_id":format!("w-{id}")},
           "payload":{"session":{"id":id,"keepalive_timeout_seconds":keepalive_secs}}})
    .to_string()
}

fn chat(message_id: &str, chat_id: &str, color: &str) -> String {
    json!({"metadata":{"message_type":"notification","message_id":message_id,
                       "message_timestamp":"2026-11-13T10:00:00Z","subscription_type":"channel.chat.message"},
           "payload":{"event":{"message_id":chat_id,"chatter_user_id":"7","chatter_user_login":"viewer",
               "chatter_user_name":"Viewer","color":color,
               "message":{"text":"hi Kappa","fragments":[
                   {"type":"text","text":"hi ","emote":null},
                   {"type":"emote","text":"Kappa","emote":{"id":"25"}}]}}}})
    .to_string()
}

async fn send(socket: &mut ws::WebSocket, text: String) {
    socket.send(ws::Message::Text(text.into())).await.unwrap();
}

async fn hold_open(mut socket: ws::WebSocket) {
    while let Some(Ok(_)) = socket.recv().await {}
}

/// First session: welcome, a duplicated chat notification, then a reconnect to `/ws2`.
async fn first(ws: WebSocketUpgrade, State(f): State<Fake>) -> Response {
    ws.on_upgrade(move |mut socket| async move {
        send(&mut socket, welcome("s1")).await;
        send(&mut socket, chat("n1", "c1", "#1E90FF")).await;
        send(&mut socket, chat("n1", "c1", "#1E90FF")).await;
        let base = f.base.lock().unwrap().replace("http://", "ws://");
        send(
            &mut socket,
            json!({"metadata":{"message_type":"session_reconnect","message_id":"r1"},
            "payload":{"session":{"id":"s1","reconnect_url":format!("{base}/ws2")}}})
            .to_string(),
        )
        .await;
        hold_open(socket).await;
    })
}

fn notification(message_id: &str, kind: &str, event: Value) -> String {
    json!({"metadata":{"message_type":"notification","message_id":message_id,
                       "message_timestamp":"2026-11-16T10:00:00Z","subscription_type":kind},
           "payload":{"event":event}})
    .to_string()
}

/// The six alert types as Twitch sends them, plus one of the per-recipient subs of a gift.
fn alert_notifications() -> Vec<String> {
    let user =
        |name: &str| json!({"user_id":"7","user_login":name.to_lowercase(),"user_name":name});
    let with = |mut base: Value, extra: Value| {
        base.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        base
    };
    vec![
        notification(
            "a1",
            "channel.follow",
            with(user("Fan"), json!({"followed_at":"2026-11-16T10:00:00Z"})),
        ),
        notification(
            "a2",
            "channel.subscribe",
            with(user("Sub"), json!({"tier":"2000","is_gift":false})),
        ),
        notification(
            "a3",
            "channel.subscription.message",
            with(
                user("Resub"),
                json!({"tier":"1000","cumulative_months":12,"streak_months":null,
                       "message":{"text":"<b>12</b> months","emotes":null}}),
            ),
        ),
        notification(
            "a4",
            "channel.subscription.gift",
            json!({"user_id":null,"user_login":null,"user_name":null,"is_anonymous":true,
                   "tier":"3000","total":20,"cumulative_total":null}),
        ),
        notification(
            "a5",
            "channel.subscribe",
            with(user("Lucky"), json!({"tier":"3000","is_gift":true})),
        ),
        notification(
            "a6",
            "channel.cheer",
            with(
                user("Cheerer"),
                json!({"is_anonymous":false,"bits":100,"message":"Cheer100 hi"}),
            ),
        ),
        notification(
            "a7",
            "channel.raid",
            json!({"from_broadcaster_user_id":"9","from_broadcaster_user_login":"raider",
                   "from_broadcaster_user_name":"Raider","viewers":42}),
        ),
        // Missing its required field: dropped with a warning, not delivered half-filled.
        notification("a8", "channel.cheer", user("Broken")),
    ]
}

/// Redemption notifications as Twitch sends them: an add, its update, an unknown status, and a broken one.
fn redemption_notifications() -> Vec<String> {
    let redemption = |id: &str, status: &str| {
        json!({"id":id,"broadcaster_user_id":"42","broadcaster_user_login":"streamer",
               "broadcaster_user_name":"Streamer","user_id":"9001","user_login":"cooler_user",
               "user_name":"Cooler_User","user_input":"pogchamp","status":status,
               "reward":{"id":"w-1","title":"Hydrate","cost":100,"prompt":"drink"},
               "redeemed_at":"2026-11-16T10:00:00.17106713Z"})
    };
    vec![
        notification("p1", REDEMPTION_ADD, redemption("r-1", "unfulfilled")),
        notification("p2", REDEMPTION_UPDATE, redemption("r-1", "fulfilled")),
        notification("p3", REDEMPTION_UPDATE, redemption("r-2", "cancelled")),
        notification("p4", REDEMPTION_ADD, redemption("r-3", "something_new")),
        // No reward: dropped with a warning.
        notification(
            "p5",
            REDEMPTION_ADD,
            json!({"id":"r-4","status":"unfulfilled"}),
        ),
    ]
}

/// Poll notifications as Twitch sends them: the start, a vote, and the end.
fn poll_notifications() -> Vec<String> {
    let choices = |votes: u64| json!([{"id":"c1","title":"晴天","votes":votes}, {"id":"c2","title":"小幸運","votes":0}]);
    vec![
        notification(
            "v1",
            POLL_BEGIN,
            json!({"id":"p1","broadcaster_user_id":"42","title":"下一首？",
                   "choices":[{"id":"c1","title":"晴天"},{"id":"c2","title":"小幸運"}],
                   "started_at":"2026-11-27T10:00:00Z","ends_at":"2026-11-27T10:02:00Z"}),
        ),
        notification(
            "v2",
            POLL_PROGRESS,
            json!({"id":"p1","title":"下一首？","choices":choices(1),
                   "started_at":"2026-11-27T10:00:00Z","ends_at":"2026-11-27T10:02:00Z"}),
        ),
        notification(
            "v3",
            POLL_END,
            json!({"id":"p1","title":"下一首？","choices":choices(1),"status":"completed",
                   "started_at":"2026-11-27T10:00:00Z","ended_at":"2026-11-27T10:02:00Z"}),
        ),
    ]
}

async fn second(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(|mut socket| async move {
        send(&mut socket, welcome("s2")).await;
        send(&mut socket, chat("n2", "c2", "red; background:url(x)")).await;
        for n in alert_notifications()
            .into_iter()
            .chain(redemption_notifications())
            .chain(poll_notifications())
        {
            send(&mut socket, n).await;
        }
        hold_open(socket).await;
    })
}

async fn subscribe(State(f): State<Fake>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    let client_id = headers
        .get("Client-Id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let kind = body["type"].as_str().map(str::to_owned);
    f.subscriptions.lock().unwrap().push((client_id, body));
    match kind.as_deref() {
        Some("channel.follow") => StatusCode::FORBIDDEN.into_response(),
        // An EventSub limit: a 429 with points left, and the bucket's reset time as on every Helix answer.
        Some("channel.subscription.gift") => {
            let reset = (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 60)
                .to_string();
            (
                StatusCode::TOO_MANY_REQUESTS,
                [
                    ("Ratelimit-Remaining", "799".to_owned()),
                    ("Ratelimit-Reset", reset),
                ],
            )
                .into_response()
        }
        _ => StatusCode::ACCEPTED.into_response(),
    }
}

async fn fake_twitch() -> (String, Fake) {
    let fake = Fake::default();
    let app = Router::new()
        .route("/ws", any(first))
        .route("/ws2", any(second))
        .route("/helix/eventsub/subscriptions", post(subscribe))
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    *fake.base.lock().unwrap() = base.clone();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, fake)
}

/// Signed in before `bits:read` was added.
pub(crate) fn signed_in_auth(http: HttpsClient) -> TwitchAuth {
    let store = Arc::new(MemoryStore::default());
    let mut scopes = crate::twitch::MVP_SCOPES.to_vec();
    scopes.retain(|s| *s != "bits:read");
    *store.value.lock().unwrap() = Some(
        json!({"access_token":"access-1","refresh_token":"refresh-1","user_id":"42",
               "login":"streamer","scopes":scopes})
        .to_string(),
    );
    TwitchAuth::new("cid".into(), "http://127.0.0.1:9", http, store)
}

async fn next<T>(rx: &mut mpsc::UnboundedReceiver<T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap()
}

#[test]
fn chat_removals_name_the_message_the_viewer_or_everything() {
    assert_eq!(
        chat_removal(
            CHAT_DELETE,
            json!({"broadcaster_user_id":"42","target_user_id":"7","message_id":"m1"})
        ),
        Some(ChatRemoval::Message { id: "m1".into() })
    );
    assert_eq!(
        chat_removal(CHAT_CLEAR_USER, json!({"target_user_id":"7"})),
        Some(ChatRemoval::User {
            user_id: "7".into()
        })
    );
    assert_eq!(
        chat_removal(CHAT_CLEAR, json!({"broadcaster_user_id":"42"})),
        Some(ChatRemoval::All)
    );
    assert_eq!(
        chat_removal(CHAT_DELETE, json!({"target_user_id":"7"})),
        None
    );
}

#[tokio::test]
async fn delivers_chat_and_alerts_once_and_follows_reconnect() {
    let (base, fake) = fake_twitch().await;
    let config = tls::client_config().unwrap();
    let http = tls::https_client(config.clone());
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (alert_tx, mut alerts) = mpsc::unbounded_channel();
    let (redemption_tx, mut redemptions) = mpsc::unbounded_channel();
    let (contest_tx, mut contests) = mpsc::unbounded_channel();
    let task = tokio::spawn(
        EventSub {
            url: format!("{}/ws", base.replace("http://", "ws://")),
            helix: Helix::new(&format!("{base}/helix"), signed_in_auth(http.clone()), http),
            tls: config,
            deliver: Arc::new(move |m| tx.send(m).unwrap()),
            deliver_alert: Arc::new(move |a| alert_tx.send(a).unwrap()),
            deliver_redemption: Arc::new(move |r| redemption_tx.send(r).unwrap()),
            deliver_stream: Arc::new(|_| {}),
            deliver_contest: Arc::new(move |c| contest_tx.send(c).unwrap()),
            deliver_removal: Arc::new(|_, _| Box::pin(async {})),
            connected: Arc::new(|_| {}),
        }
        .run(),
    );
    let first = next(&mut rx).await;
    assert_eq!(first.id, "c1");
    assert_eq!(first.user_name, "Viewer");
    assert_eq!(first.color.as_deref(), Some("#1E90FF"));
    assert_eq!(first.sent_at, "2026-11-13T10:00:00Z");
    assert_eq!(
        first.fragments,
        vec![
            Fragment::Text("hi ".into()),
            Fragment::Emote {
                id: "25".into(),
                text: "Kappa".into()
            }
        ]
    );
    // The duplicate n1 is dropped, so the next delivery comes from the reconnected session.
    let second = next(&mut rx).await;
    assert_eq!(second.id, "c2");
    assert_eq!(second.color, None, "invalid colors are dropped");

    let mut received = Vec::new();
    for _ in 0..7 {
        received.push(next(&mut alerts).await);
    }
    let named = |name: &str| Some(name.to_owned());
    let expected = [
        ("a1", named("Fan"), AlertKind::Follow),
        (
            "a2",
            named("Sub"),
            AlertKind::Subscribe {
                tier: 2,
                gifted: false,
            },
        ),
        (
            "a3",
            named("Resub"),
            AlertKind::Resubscribe {
                tier: 1,
                months: 12,
                message: "<b>12</b> months".into(),
            },
        ),
        ("a4", None, AlertKind::Gift { tier: 3, count: 20 }),
        (
            "a5",
            named("Lucky"),
            AlertKind::Subscribe {
                tier: 3,
                gifted: true,
            },
        ),
        (
            "a6",
            named("Cheerer"),
            AlertKind::Cheer {
                bits: 100,
                message: "Cheer100 hi".into(),
            },
        ),
        ("a7", named("Raider"), AlertKind::Raid { viewers: 42 }),
    ];
    for (alert, (id, user_name, kind)) in received.iter().zip(expected) {
        assert_eq!(alert.id, id);
        assert_eq!(alert.user_name, user_name, "{id}");
        // Each fixture's login is its name in lower case; the raider's ID is 9, everyone else's 7.
        let login = user_name.as_ref().map(|n| n.to_lowercase());
        assert_eq!(alert.user_login, login, "{id}");
        let user_id = user_name
            .as_ref()
            .map(|n| if n == "Raider" { "9" } else { "7" }.to_owned());
        assert_eq!(alert.user_id, user_id, "{id}");
        assert_eq!(alert.kind, kind, "{id}");
        assert_eq!(alert.occurred_at, "2026-11-16T10:00:00Z");
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(200), alerts.recv())
            .await
            .is_err(),
        "the broken cheer is not delivered"
    );

    let first = next(&mut redemptions).await;
    assert_eq!(
        first,
        ChannelPointsRedemption {
            id: "r-1".into(),
            change: RedemptionChange::Added,
            user_id: "9001".into(),
            user_login: "cooler_user".into(),
            user_name: "Cooler_User".into(),
            reward_id: "w-1".into(),
            reward_title: "Hydrate".into(),
            reward_cost: 100,
            user_input: "pogchamp".into(),
            status: RedemptionStatus::Unfulfilled,
            redeemed_at: "2026-11-16T10:00:00.17106713Z".into(),
        }
    );
    let mut rest = Vec::new();
    for _ in 0..3 {
        let r = next(&mut redemptions).await;
        rest.push((r.id, r.change, r.status));
    }
    assert_eq!(
        rest,
        [
            (
                "r-1".into(),
                RedemptionChange::Updated,
                RedemptionStatus::Fulfilled
            ),
            (
                "r-2".into(),
                RedemptionChange::Updated,
                RedemptionStatus::Canceled
            ),
            (
                "r-3".into(),
                RedemptionChange::Added,
                RedemptionStatus::Unknown
            ),
        ]
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(200), redemptions.recv())
            .await
            .is_err(),
        "the broken redemption is not delivered"
    );
    // A poll: each notification carries all of it.
    let mut polls = Vec::new();
    for _ in 0..3 {
        let Contest::Poll(p) = next(&mut contests).await;
        polls.push((p.status, p.choices[0].votes, p.ends_at));
    }
    assert_eq!(
        polls,
        [
            (PollStatus::Active, 0, "2026-11-27T10:02:00Z".into()),
            (PollStatus::Active, 1, "2026-11-27T10:02:00Z".into()),
            (PollStatus::Completed, 1, "2026-11-27T10:02:00Z".into()),
        ]
    );

    let subs = fake.subscriptions.lock().unwrap().clone();
    let types: Vec<&str> = subs
        .iter()
        .map(|(_, b)| b["type"].as_str().unwrap())
        .collect();
    // Subscriptions carry over a reconnect; cheer needs bits:read, which this token lacks; the refused
    // follow and the gift refused by an EventSub limit did not stop the rest.
    assert_eq!(
        types,
        [
            "channel.chat.message",
            "channel.follow",
            "channel.subscribe",
            "channel.subscription.message",
            "channel.subscription.gift",
            "channel.raid",
            REDEMPTION_ADD,
            REDEMPTION_UPDATE,
            STREAM_ONLINE,
            STREAM_OFFLINE,
            CHAT_DELETE,
            CHAT_CLEAR_USER,
            CHAT_CLEAR
        ]
    );
    for sub in &subs[8..10] {
        assert_eq!(sub.1["condition"], json!({"broadcaster_user_id":"42"}));
    }
    for sub in &subs[10..] {
        assert_eq!(
            sub.1["condition"],
            json!({"broadcaster_user_id":"42","user_id":"42"})
        );
    }
    assert_eq!(
        subs[1].1["condition"],
        json!({"broadcaster_user_id":"42","moderator_user_id":"42"})
    );
    assert_eq!(subs[1].1["version"], "2");
    assert_eq!(
        subs[5].1["condition"],
        json!({"to_broadcaster_user_id":"42"})
    );
    let (client_id, body) = &subs[0];
    assert_eq!(client_id.as_deref(), Some("cid"));
    assert_eq!(body["type"], "channel.chat.message");
    assert_eq!(
        body["condition"],
        json!({"broadcaster_user_id":"42","user_id":"42"})
    );
    assert_eq!(
        body["transport"],
        json!({"method":"websocket","session_id":"s1"})
    );
    task.abort();
}

/// One behavior for every connection: a welcome (session `s<n>`, 1 s keepalive), then `after_welcome` if
/// any, then silence. The chat subscription answers `chat_status`; the others are accepted, unless
/// `raid_rate_limit_reset` makes the raid subscription hit an empty points bucket.
#[derive(Clone)]
struct Scripted {
    after_welcome: Option<String>,
    chat_status: StatusCode,
    rate_limit_reset: Option<u64>,
    raid_rate_limit_reset: Option<u64>,
    connections: Arc<Mutex<Vec<std::time::Instant>>>,
    /// Session IDs the chat subscription was requested for.
    chat_sessions: Arc<Mutex<Vec<String>>>,
    closed: Arc<std::sync::atomic::AtomicU32>,
}

impl Scripted {
    fn new(after_welcome: Option<String>, chat_status: StatusCode) -> Self {
        Self {
            after_welcome,
            chat_status,
            rate_limit_reset: None,
            raid_rate_limit_reset: None,
            connections: Arc::default(),
            chat_sessions: Arc::default(),
            closed: Arc::default(),
        }
    }

    fn connections(&self) -> Vec<std::time::Instant> {
        self.connections.lock().unwrap().clone()
    }

    fn chat_sessions(&self) -> Vec<String> {
        self.chat_sessions.lock().unwrap().clone()
    }

    fn closed(&self) -> u32 {
        self.closed.load(std::sync::atomic::Ordering::SeqCst)
    }
}

async fn scripted_ws(ws: WebSocketUpgrade, State(f): State<Scripted>) -> Response {
    ws.on_upgrade(move |mut socket| async move {
        let n = {
            let mut connections = f.connections.lock().unwrap();
            connections.push(std::time::Instant::now());
            connections.len()
        };
        send(&mut socket, welcome_with(&format!("s{n}"), 1)).await;
        if let Some(text) = f.after_welcome.clone() {
            send(&mut socket, text).await;
        }
        hold_open(socket).await;
        f.closed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    })
}

async fn scripted_subscribe(State(f): State<Scripted>, Json(body): Json<Value>) -> Response {
    use axum::response::IntoResponse;
    if body["type"] == "channel.raid"
        && let Some(reset) = f.raid_rate_limit_reset
    {
        let headers = [
            ("Ratelimit-Remaining", "0".to_owned()),
            ("Ratelimit-Reset", reset.to_string()),
        ];
        return (StatusCode::TOO_MANY_REQUESTS, headers).into_response();
    }
    if body["type"] != "channel.chat.message" {
        return StatusCode::ACCEPTED.into_response();
    }
    let session = body["transport"]["session_id"].as_str().unwrap_or_default();
    f.chat_sessions.lock().unwrap().push(session.into());
    match f.rate_limit_reset {
        Some(reset) => (f.chat_status, [("Ratelimit-Reset", reset.to_string())]).into_response(),
        None => f.chat_status.into_response(),
    }
}

/// Runs EventSub against `script`; returns the task and the sign-in it uses.
/// The task, its sign-in, and every `connected` report in order.
async fn run_scripted(
    script: &Scripted,
) -> (
    tokio::task::JoinHandle<()>,
    TwitchAuth,
    Arc<std::sync::Mutex<Vec<bool>>>,
) {
    let app = Router::new()
        .route("/ws", any(scripted_ws))
        .route("/helix/eventsub/subscriptions", post(scripted_subscribe))
        .with_state(script.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let config = tls::client_config().unwrap();
    let http = tls::https_client(config.clone());
    let auth = signed_in_auth(http.clone());
    let reports = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = reports.clone();
    let task = tokio::spawn(
        EventSub {
            url: format!("{}/ws", base.replace("http://", "ws://")),
            helix: Helix::new(&format!("{base}/helix"), auth.clone(), http),
            tls: config,
            deliver: Arc::new(|_| {}),
            deliver_alert: Arc::new(|_| {}),
            deliver_redemption: Arc::new(|_| {}),
            deliver_stream: Arc::new(|_| {}),
            deliver_contest: Arc::new(|_| {}),
            deliver_removal: Arc::new(|_, _| Box::pin(async {})),
            connected: Arc::new(move |c| log.lock().unwrap().push(c)),
        }
        .run(),
    );
    (task, auth, reports)
}

async fn until(what: &str, done: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(15), async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
}

#[tokio::test]
async fn silence_past_the_keepalive_reconnects_and_subscribes_again() {
    let script = Scripted::new(None, StatusCode::ACCEPTED);
    let (task, _auth, reports) = run_scripted(&script).await;
    until("a second session", || script.chat_sessions().len() >= 2).await;
    assert_eq!(script.chat_sessions()[..2], ["s1", "s2"]);
    until("connected again", || reports.lock().unwrap().len() >= 3).await;
    assert_eq!(reports.lock().unwrap()[..3], [true, false, true]);
    let c = script.connections();
    // 1 s keepalive + 1 s grace, then a 1 s backoff.
    assert!(c[1] - c[0] >= Duration::from_secs(3), "{:?}", c[1] - c[0]);
    task.abort();
}

#[tokio::test]
async fn a_revocation_ends_the_session_and_a_new_one_subscribes() {
    let revocation = json!({"metadata":{"message_type":"revocation","message_id":"v1"},
        "payload":{"subscription":{"type":"channel.chat.message","status":"authorization_revoked"}}})
    .to_string();
    let script = Scripted::new(Some(revocation), StatusCode::ACCEPTED);
    let (task, _auth, _) = run_scripted(&script).await;
    until("a second session", || script.chat_sessions().len() >= 2).await;
    assert_eq!(script.chat_sessions()[..2], ["s1", "s2"]);
    until("the revoked session closed", || script.closed() >= 1).await;
    let c = script.connections();
    // Ended by the revocation, long before the keepalive would; then a 1 s backoff.
    assert!(c[1] - c[0] < Duration::from_secs(3), "{:?}", c[1] - c[0]);
    task.abort();
}

#[tokio::test]
async fn logout_closes_the_session_and_stays_away() {
    let script = Scripted::new(None, StatusCode::ACCEPTED);
    let (task, auth, reports) = run_scripted(&script).await;
    until("the first session", || script.chat_sessions().len() == 1).await;
    until("connected", || {
        reports.lock().unwrap().first() == Some(&true)
    })
    .await;
    auth.logout().unwrap();
    until("the session closed", || script.closed() == 1).await;
    assert_eq!(reports.lock().unwrap().last(), Some(&false));
    // Longer than a keepalive timeout plus a backoff: no new session while signed out.
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert_eq!(script.connections().len(), 1);
    assert!(!task.is_finished(), "waits for the next sign-in");
    task.abort();
}

#[tokio::test]
async fn a_refused_chat_subscription_backs_off_longer_each_time() {
    let script = Scripted::new(None, StatusCode::FORBIDDEN);
    let (task, _auth, reports) = run_scripted(&script).await;
    until("three sessions", || script.connections().len() >= 3).await;
    assert!(
        !reports.lock().unwrap().contains(&true),
        "a session whose chat subscription is refused is not connected"
    );
    let c = script.connections();
    assert!(c[1] - c[0] >= Duration::from_secs(1), "{:?}", c[1] - c[0]);
    assert!(c[2] - c[1] >= Duration::from_secs(2), "{:?}", c[2] - c[1]);
    task.abort();
}

#[tokio::test]
async fn a_rate_limited_chat_subscription_waits_for_the_reset() {
    let mut script = Scripted::new(None, StatusCode::TOO_MANY_REQUESTS);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    script.rate_limit_reset = Some(now + 4);
    let (task, _auth, _) = run_scripted(&script).await;
    until("a second session", || script.connections().len() >= 2).await;
    let c = script.connections();
    // Longer than the 1 s backoff: at least until the reset, 3 to 4 s away when the 429 came.
    assert!(c[1] - c[0] >= Duration::from_secs(2), "{:?}", c[1] - c[0]);
    task.abort();
}

#[tokio::test]
async fn an_empty_points_bucket_mid_subscribing_waits_for_the_reset_and_starts_over() {
    let mut script = Scripted::new(None, StatusCode::ACCEPTED);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    script.raid_rate_limit_reset = Some(now + 4);
    let (task, _auth, reports) = run_scripted(&script).await;
    until("a second session", || script.connections().len() >= 2).await;
    let c = script.connections();
    // The rest are not skipped: the session ends and a new one subscribes after the reset.
    assert!(c[1] - c[0] >= Duration::from_secs(2), "{:?}", c[1] - c[0]);
    assert!(
        !reports.lock().unwrap().contains(&true),
        "a session that could not subscribe is not connected"
    );
    task.abort();
}

fn chat_event(extra: Value) -> ChatMessage {
    let mut event = json!({"message_id":"c","chatter_user_id":"7","chatter_user_login":"viewer",
                           "chatter_user_name":"Viewer","color":"","message":{"text":"hi","fragments":[]}});
    event
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value::<ChatEvent>(event)
        .unwrap()
        .into_message("2026-11-19T10:00:00Z".into())
}

#[test]
fn chat_carries_roles_highlight_bits_and_reply() {
    let plain = chat_event(json!({"message_type":"text","badges":[],"cheer":null,"reply":null}));
    assert_eq!(
        (plain.badges, plain.highlighted, plain.bits, plain.reply),
        (vec![], false, 0, None)
    );

    let full = chat_event(json!({
        "message_type":"channel_points_highlighted",
        "badges":[{"set_id":"moderator","id":"1","info":""},{"set_id":"founder","id":"0","info":"3"},
                  {"set_id":"subscriber","id":"12","info":"12"},{"set_id":"turbo","id":"1","info":""}],
        "cheer":{"bits":250},
        "reply":{"parent_message_id":"p","parent_message_body":"hello","parent_user_id":"8",
                 "parent_user_login":"other","parent_user_name":"Other","thread_message_id":"p",
                 "thread_user_id":"8","thread_user_login":"other","thread_user_name":"Other"}}));
    // Founder and subscriber are one role; Turbo is not a role.
    assert_eq!(
        full.badges,
        vec![ChatBadge::Moderator, ChatBadge::Subscriber]
    );
    assert!(full.highlighted);
    assert_eq!(full.bits, 250);
    assert_eq!(
        full.reply,
        Some(ChatReply {
            message_id: "p".into(),
            user_id: "8".into(),
            user_login: "other".into(),
            user_name: "Other".into(),
        })
    );

    for (set_id, role) in [
        ("broadcaster", ChatBadge::Broadcaster),
        ("vip", ChatBadge::Vip),
    ] {
        let m = chat_event(json!({"badges":[{"set_id":set_id,"id":"1","info":""}]}));
        assert_eq!(m.badges, vec![role]);
    }
}

#[test]
fn chat_keeps_every_badge_and_where_a_shared_chat_message_came_from() {
    let m = chat_event(json!({
        "broadcaster_user_id":"42",
        "badges":[{"set_id":"subscriber","id":"3012","info":"16"},{"set_id":"turbo","id":"1","info":""}],
        "source_broadcaster_user_id":"99","source_broadcaster_user_login":"pianocat",
        "source_broadcaster_user_name":"PianoCat"}));
    assert_eq!(
        m.twitch_badges,
        [
            TwitchBadge {
                set_id: "subscriber".into(),
                id: "3012".into(),
                info: "16".into()
            },
            TwitchBadge {
                set_id: "turbo".into(),
                id: "1".into(),
                info: String::new()
            }
        ]
    );
    assert_eq!(
        m.shared_from,
        Some(SharedChannel {
            user_id: "99".into(),
            login: "pianocat".into(),
            name: "PianoCat".into()
        })
    );
    // This channel's own messages during a shared chat, and chat outside one, have no other channel.
    let own = chat_event(
        json!({"broadcaster_user_id":"42","source_broadcaster_user_id":"42",
        "source_broadcaster_user_login":"me","source_broadcaster_user_name":"Me"}),
    );
    assert_eq!(own.shared_from, None);
    let plain = chat_event(json!({"broadcaster_user_id":"42","source_broadcaster_user_id":null}));
    assert_eq!(plain.shared_from, None);
}

#[test]
fn an_anonymous_cheer_has_no_viewer() {
    let event: AlertEvent = serde_json::from_value(json!({
        "is_anonymous":true,"user_id":null,"user_login":null,"user_name":null,"bits":100,"message":"Cheer100"
    }))
    .unwrap();
    let alert = event
        .into_alert("channel.cheer", "a".into(), "2026-11-19T10:00:00Z".into())
        .unwrap();
    assert_eq!(
        (alert.user_id, alert.user_login, alert.user_name),
        (None, None, None)
    );
}

#[test]
fn a_live_start_and_an_end_reach_the_stream_module() {
    let online = |kind: &str| {
        json!({"id": "9001", "broadcaster_user_id": "42", "broadcaster_user_login": "me",
               "broadcaster_user_name": "Me", "type": kind, "started_at": "2026-11-19T10:00:00.123Z"})
    };
    assert_eq!(
        stream_change(STREAM_ONLINE, online("live")),
        Some(StreamChange::Online {
            id: "9001".into(),
            started_at: "2026-11-19T10:00:00.123Z".into()
        })
    );
    assert_eq!(
        stream_change(STREAM_ONLINE, online("rerun")),
        None,
        "only live streams count"
    );
    assert_eq!(stream_change(STREAM_ONLINE, json!({"id": "9001"})), None);
    assert_eq!(
        stream_change(STREAM_OFFLINE, json!({"broadcaster_user_id": "42"})),
        Some(StreamChange::Offline)
    );
}

#[test]
fn only_tls_or_loopback_endpoints() {
    assert!(allowed_endpoint(EVENTSUB_URL));
    assert!(allowed_endpoint("ws://127.0.0.1:1234/ws"));
    assert!(!allowed_endpoint("ws://eventsub.wss.twitch.tv/ws"));
    assert!(!allowed_endpoint("ws://127.0.0.1@evil.example/ws"));
    assert!(!allowed_endpoint("https://eventsub.wss.twitch.tv/ws"));
}
