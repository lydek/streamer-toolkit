//! A fake Twitch: the Device Code Flow (approved at once), the Helix endpoints the app calls, an EventSub
//! WebSocket session and anonymous IRC chat, under `/oauth2`, `/helix`, `/eventsub` and `/irc` of one port.
//! Users have no avatar, so the app's Twitch image check stays as it is.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Form, Json, Router,
    body::{Body, to_bytes},
    extract::{RawQuery, Request, State, WebSocketUpgrade, ws},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
};
use serde_json::{Value, json};
use tokio::{net::TcpListener, sync::broadcast};

use crate::metrics::{self, Call, Recorder};

pub const BROADCASTER_ID: &str = "1000";
pub const BROADCASTER_LOGIN: &str = "fake_streamer";
/// The access token of `integrations::twitch::auth::fake_stored_token` (a fakes test checks they match).
pub const PRELOADED_TOKEN: &str = "fake-access";
const BROADCASTER_NAME: &str = "Fake_Streamer";
const TIMESTAMP: &str = "2026-11-26T12:00:00Z";

struct World {
    /// Scopes asked for at the last device login; `/validate` grants them.
    scopes: Vec<String>,
    live: bool,
    subscriptions: Vec<Value>,
    eventsub_connects: u32,
    irc_joins: Vec<String>,
    next_id: u64,
    /// Access tokens this fake issued; any other is an unknown token.
    tokens: HashSet<String>,
}

struct Inner {
    world: Mutex<World>,
    /// Notification messages for every EventSub session.
    eventsub: broadcast::Sender<String>,
    /// Ends every EventSub session, as a dropped connection would.
    close: broadcast::Sender<()>,
    /// IRC lines for every chat connection.
    irc: broadcast::Sender<String>,
    keepalive: Duration,
    recorder: Recorder,
}

#[derive(Clone)]
pub struct FakeTwitch(Arc<Inner>);

impl Default for FakeTwitch {
    fn default() -> Self {
        Self::new(Duration::from_secs(10))
    }
}

impl FakeTwitch {
    /// `keepalive` is the EventSub session's keepalive timeout; the fake sends a keepalive every half of it.
    pub fn new(keepalive: Duration) -> Self {
        Self::with_recorder(keepalive, Recorder::default())
    }

    /// Logs requests and events into `recorder`, shared with the fake OBS for one timeline.
    pub fn with_recorder(keepalive: Duration, recorder: Recorder) -> Self {
        Self(Arc::new(Inner {
            world: Mutex::new(World {
                scopes: Vec::new(),
                live: false,
                subscriptions: Vec::new(),
                eventsub_connects: 0,
                irc_joins: Vec::new(),
                next_id: 0,
                tokens: HashSet::from([PRELOADED_TOKEN.to_owned()]),
            }),
            eventsub: broadcast::channel(1024).0,
            close: broadcast::channel(1).0,
            irc: broadcast::channel(1024).0,
            keepalive,
            recorder,
        }))
    }

    pub fn recorder(&self) -> &Recorder {
        &self.0.recorder
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/oauth2/device", post(device))
            .route("/oauth2/token", post(token))
            .route("/oauth2/validate", get(validate))
            .route("/helix/users", get(users))
            .route("/helix/streams", get(streams))
            .route("/helix/channel_points/custom_rewards", get(rewards))
            .route("/helix/chat/badges/global", get(badges))
            .route("/helix/chat/badges", get(badges))
            .route(
                "/helix/eventsub/subscriptions",
                post(subscribe).get(subscriptions),
            )
            .route("/eventsub", any(eventsub))
            .route("/irc", any(irc))
            .layer(middleware::from_fn_with_state(self.clone(), record))
            .with_state(self.clone())
    }

    /// Serves until the listener fails; bind it to `127.0.0.1` only.
    pub async fn serve(self, listener: TcpListener) -> std::io::Result<()> {
        axum::serve(listener, self.router()).await
    }

    /// The EventSub subscriptions created so far, as their request bodies.
    pub fn subscriptions(&self) -> Vec<Value> {
        self.0.world.lock().unwrap().subscriptions.clone()
    }

    pub fn eventsub_connects(&self) -> u32 {
        self.0.world.lock().unwrap().eventsub_connects
    }

    /// Channels joined over IRC.
    pub fn irc_joins(&self) -> Vec<String> {
        self.0.world.lock().unwrap().irc_joins.clone()
    }

    /// Whether `/helix/streams` reports the broadcaster live.
    pub fn set_live(&self, live: bool) {
        self.0.world.lock().unwrap().live = live;
    }

    /// Sends `event` as a notification of `kind` to every EventSub session, as Twitch does only for a type the app
    /// subscribed to: `false` (nothing sent) when there is no such subscription.
    pub fn notify(&self, kind: &str, event: Value) -> bool {
        let Some(subscription) = self
            .subscriptions()
            .into_iter()
            .rev()
            .find(|s| s["type"] == kind)
        else {
            return false;
        };
        let message = json!({"metadata": {"message_id": self.id("message"), "message_type": "notification",
                                          "message_timestamp": now(), "subscription_type": kind,
                                          "subscription_version": subscription["version"]},
                             "payload": {"subscription": subscription, "event": event}});
        self.0.recorder.event("twitch", kind);
        let _ = self.0.eventsub.send(message.to_string());
        true
    }

    /// Twitch's first-chat mark over IRC for the chat message `id`.
    pub fn first_chat(&self, id: &str, login: &str, text: &str) {
        let line = format!(
            "@first-msg=1;id={id} :{login}!{login}@{login}.tmi.twitch.tv PRIVMSG #{BROADCASTER_LOGIN} :{text}\r\n"
        );
        let _ = self.0.irc.send(line);
    }

    /// Ends every EventSub session; the app connects again on its own.
    pub fn drop_eventsub(&self) {
        let _ = self.0.close.send(());
    }

    pub(crate) fn id(&self, prefix: &str) -> String {
        let mut w = self.0.world.lock().unwrap();
        w.next_id += 1;
        format!("{prefix}-{}", w.next_id)
    }
}

/// The current UTC time as RFC 3339, as Twitch stamps messages.
pub(crate) fn now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    rfc3339(secs)
}

fn rfc3339(secs: u64) -> String {
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

fn user(id: &str, login: &str) -> Value {
    json!({"id": id, "login": login, "display_name": login, "type": "", "broadcaster_type": "affiliate",
           "description": "", "profile_image_url": "", "offline_image_url": "", "created_at": TIMESTAMP})
}

/// A stable digit ID for a login, so the same viewer keeps one ID.
pub(crate) fn id_for(login: &str) -> String {
    if login == BROADCASTER_LOGIN {
        return BROADCASTER_ID.into();
    }
    let n = login
        .bytes()
        .fold(0u64, |n, b| (n * 31 + b as u64) % 1_000_000);
    (2_000_000 + n).to_string()
}

async fn device(
    State(fake): State<FakeTwitch>,
    Form(form): Form<Vec<(String, String)>>,
) -> Response {
    let scopes = form
        .iter()
        .find(|(k, _)| k == "scopes")
        .map(|(_, v)| v.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default();
    fake.0.world.lock().unwrap().scopes = scopes;
    Json(json!({"device_code": fake.id("device"), "user_code": "FAKE-CODE",
                "verification_uri": "https://www.twitch.tv/activate", "expires_in": 1800, "interval": 1}))
    .into_response()
}

/// Every device code is approved at once; refresh tokens always work.
async fn token(State(fake): State<FakeTwitch>) -> Response {
    let scopes = fake.0.world.lock().unwrap().scopes.clone();
    let access = fake.id("access");
    fake.0.world.lock().unwrap().tokens.insert(access.clone());
    Json(
        json!({"access_token": access, "refresh_token": fake.id("refresh"),
                "expires_in": 14400, "scope": scopes, "token_type": "bearer"}),
    )
    .into_response()
}

async fn validate(State(fake): State<FakeTwitch>, headers: HeaderMap) -> Response {
    let authorized = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("OAuth "));
    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"status": 401, "message": "invalid access token"})),
        )
            .into_response();
    }
    let scopes = fake.0.world.lock().unwrap().scopes.clone();
    Json(
        json!({"client_id": "", "login": BROADCASTER_LOGIN, "scopes": scopes,
                "user_id": BROADCASTER_ID, "expires_in": 14400}),
    )
    .into_response()
}

/// `?id=…&login=…` (repeatable); no parameters means the signed-in broadcaster.
async fn users(RawQuery(query): RawQuery) -> Response {
    let mut data = Vec::new();
    for pair in query.as_deref().unwrap_or_default().split('&') {
        match pair.split_once('=') {
            Some(("id", id)) if id == BROADCASTER_ID => data.push(user(id, BROADCASTER_LOGIN)),
            Some(("id", id)) => data.push(user(id, &format!("viewer_{id}"))),
            Some(("login", login)) => data.push(user(&id_for(login), login)),
            _ => {}
        }
    }
    if data.is_empty() && query.as_deref().is_none_or(str::is_empty) {
        data.push(user(BROADCASTER_ID, BROADCASTER_LOGIN));
    }
    Json(json!({ "data": data })).into_response()
}

async fn streams(State(fake): State<FakeTwitch>) -> Response {
    let live = fake.0.world.lock().unwrap().live;
    let data = if live {
        json!([{"id": "fake-stream", "user_id": BROADCASTER_ID, "user_login": BROADCASTER_LOGIN,
                "user_name": BROADCASTER_NAME, "game_name": "Just Chatting", "type": "live",
                "title": "Fake stream", "viewer_count": 42, "started_at": TIMESTAMP}])
    } else {
        json!([])
    };
    Json(json!({ "data": data })).into_response()
}

async fn rewards() -> Response {
    let reward = |id: &str, title: &str, cost: u32| {
        json!({"broadcaster_id": BROADCASTER_ID, "broadcaster_login": BROADCASTER_LOGIN,
               "broadcaster_name": BROADCASTER_NAME, "id": id, "title": title, "prompt": "", "cost": cost,
               "image": null, "default_image": null, "background_color": "#9147FF", "is_enabled": true,
               "is_user_input_required": false, "is_paused": false, "is_in_stock": true,
               "should_redemptions_skip_request_queue": false})
    };
    Json(json!({"data": [
        reward("fake-reward-check-in", "Check in", 100),
        reward("fake-reward-hydrate", "Hydrate", 500),
        reward("fake-reward-play-a-sound", "Play a sound", 1000),
    ]}))
    .into_response()
}

/// Twitch's own badge images (public CDN); offline they simply do not load.
async fn badges() -> Response {
    let version = |id: &str, image: &str, title: &str| {
        let base = format!("https://static-cdn.jtvnw.net/badges/v1/{image}");
        json!({"id": id, "image_url_1x": format!("{base}/1"), "image_url_2x": format!("{base}/2"),
               "image_url_4x": format!("{base}/3"), "title": title, "description": title})
    };
    Json(json!({"data": [
        {"set_id": "moderator", "versions": [version("1", "3267646d-33f0-4b17-b3df-f923a41db1d0", "Moderator")]},
        {"set_id": "subscriber", "versions": [version("0", "5d9f2208-5dd8-11e7-8513-2ff4adfae661", "Subscriber")]},
    ]}))
    .into_response()
}

async fn subscribe(State(fake): State<FakeTwitch>, Json(body): Json<Value>) -> Response {
    let id = fake.id("subscription");
    let mut row = body.clone();
    row["id"] = json!(id);
    row["status"] = json!("enabled");
    row["created_at"] = json!(TIMESTAMP);
    row["cost"] = json!(0);
    fake.0.world.lock().unwrap().subscriptions.push(row.clone());
    (
        StatusCode::ACCEPTED,
        Json(json!({"data": [row], "total": 1, "total_cost": 0, "max_total_cost": 10000})),
    )
        .into_response()
}

async fn subscriptions(State(fake): State<FakeTwitch>) -> Response {
    let rows = fake.subscriptions();
    Json(
        json!({"data": rows, "total": rows.len(), "total_cost": 0, "max_total_cost": 10000,
                "pagination": {}}),
    )
    .into_response()
}

async fn send(socket: &mut ws::WebSocket, text: String) -> bool {
    socket.send(ws::Message::Text(text.into())).await.is_ok()
}

async fn eventsub(upgrade: WebSocketUpgrade, State(fake): State<FakeTwitch>) -> Response {
    upgrade.on_upgrade(move |mut socket| async move {
        let session = fake.id("session");
        fake.0.world.lock().unwrap().eventsub_connects += 1;
        fake.0.recorder.connect("twitch", "eventsub");
        let keepalive = fake.0.keepalive;
        let welcome = json!({"metadata": {"message_id": fake.id("message"), "message_type": "session_welcome",
                                          "message_timestamp": TIMESTAMP},
            "payload": {"session": {"id": session, "status": "connected", "connected_at": TIMESTAMP,
                                    "keepalive_timeout_seconds": keepalive.as_secs().max(1),
                                    "reconnect_url": null}}});
        if !send(&mut socket, welcome.to_string()).await {
            return;
        }
        let mut notifications = fake.0.eventsub.subscribe();
        let mut close = fake.0.close.subscribe();
        let mut every = tokio::time::interval(keepalive / 2);
        every.tick().await;
        loop {
            tokio::select! {
                _ = close.recv() => {
                    let _ = socket.send(ws::Message::Close(None)).await;
                    return;
                }
                _ = every.tick() => {
                    let message = json!({"metadata": {"message_id": fake.id("message"),
                        "message_type": "session_keepalive", "message_timestamp": TIMESTAMP}, "payload": {}});
                    if !send(&mut socket, message.to_string()).await {
                        return;
                    }
                }
                notification = notifications.recv() => match notification {
                    Ok(text) => if !send(&mut socket, text).await { return },
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                },
                message = socket.recv() => match message {
                    Some(Ok(_)) => {}
                    _ => return,
                },
            }
        }
    })
}

/// What is wrong with a request's headers for its endpoint, if anything.
fn header_problem(path: &str, headers: &HeaderMap, tokens: &HashSet<String>) -> Option<String> {
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
    };
    let token = |scheme: &str| -> Option<String> {
        let Some(token) = header("Authorization").strip_prefix(scheme) else {
            return Some(format!("missing Authorization: {}…", scheme));
        };
        (!tokens.contains(token)).then(|| "unknown access token".to_owned())
    };
    if path.starts_with("/helix/") {
        if header("Client-Id").is_empty() {
            return Some("missing Client-Id".into());
        }
        return token("Bearer ");
    }
    if path == "/oauth2/validate" {
        return token("OAuth ");
    }
    // `/oauth2/device` and `/oauth2/token` carry `client_id` in the form body.
    None
}

/// Logs every `/oauth2` and `/helix` request: its key, parameters, status, handling time and header problems.
async fn record(State(fake): State<FakeTwitch>, request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    if !path.starts_with("/oauth2/") && !path.starts_with("/helix/") {
        return next.run(request).await;
    }
    let key = format!("{} {path}", request.method());
    let query = request.uri().query().unwrap_or_default().to_owned();
    let problem = {
        let tokens = &fake.0.world.lock().unwrap().tokens;
        header_problem(&path, request.headers(), tokens)
    };
    let (parts, body) = request.into_parts();
    let Ok(body) = to_bytes(body, 1 << 20).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    let params = metrics::hash(&[query.as_bytes(), &body]);
    let in_flight = fake.0.recorder.start("twitch");
    let started = Instant::now();
    let response = next.run(Request::from_parts(parts, Body::from(body))).await;
    fake.0.recorder.call(Call {
        at: started,
        service: "twitch",
        key,
        params,
        status: response.status().as_u16(),
        handled: started.elapsed(),
        in_flight: in_flight.count,
        problem,
    });
    response
}

/// Anonymous IRC: acknowledges the capabilities, confirms the join and pings now and then.
async fn irc(upgrade: WebSocketUpgrade, State(fake): State<FakeTwitch>) -> Response {
    upgrade.on_upgrade(move |mut socket| async move {
        fake.0.recorder.connect("twitch", "irc");
        let mut nick = String::from("justinfan");
        let mut lines = fake.0.irc.subscribe();
        let mut every = tokio::time::interval(fake.0.keepalive / 2);
        every.tick().await;
        loop {
            tokio::select! {
                _ = every.tick() => if !send(&mut socket, "PING :tmi.twitch.tv\r\n".into()).await { return },
                line = lines.recv() => match line {
                    Ok(line) => if !send(&mut socket, line).await { return },
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                },
                message = socket.recv() => {
                    let Some(Ok(ws::Message::Text(text))) = message else {
                        if matches!(message, Some(Ok(_))) { continue }
                        return;
                    };
                    for line in text.as_str().split("\r\n").filter(|l| !l.is_empty()) {
                        let reply = if let Some(caps) = line.strip_prefix("CAP REQ :") {
                            format!(":tmi.twitch.tv CAP * ACK :{caps}\r\n")
                        } else if let Some(n) = line.strip_prefix("NICK ") {
                            nick = n.trim().into();
                            continue;
                        } else if let Some(channel) = line.strip_prefix("JOIN #") {
                            fake.0.world.lock().unwrap().irc_joins.push(channel.trim().into());
                            format!(":{nick}!{nick}@{nick}.tmi.twitch.tv JOIN #{}\r\n", channel.trim())
                        } else {
                            continue;
                        };
                        if !send(&mut socket, reply).await {
                            return;
                        }
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_rfc3339_utc() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_795_700_000), "2026-11-26T13:33:20Z");
        assert_eq!(now().len(), 20);
    }

    #[test]
    fn viewer_ids_are_digits_and_stable() {
        assert_eq!(id_for(BROADCASTER_LOGIN), BROADCASTER_ID);
        let id = id_for("viewer_a");
        assert_eq!(id, id_for("viewer_a"));
        assert_ne!(id, id_for("viewer_b"));
        assert!(id.bytes().all(|b| b.is_ascii_digit()));
    }

    #[test]
    fn users_have_no_avatar() {
        assert_eq!(user("1", "a")["profile_image_url"], "");
    }
}
