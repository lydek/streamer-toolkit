//! EventSub over WebSocket: one session per signed-in account, reconnecting with backoff. Chat plus the
//! follow, subscription, cheer and raid alerts.

use std::{collections::VecDeque, convert::Infallible, sync::Arc, time::Duration};

use futures_util::StreamExt;
use rustls::ClientConfig;
use serde::Deserialize;
use tokio::net::TcpStream;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream, tungstenite};
use toolkit_core::events::{Alert, AlertKind, ChatMessage, Fragment};

use super::{
    auth::AuthError,
    helix::{Helix, HelixError},
};

pub const EVENTSUB_URL: &str = "wss://eventsub.wss.twitch.tv/ws";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Twitch sends something at least every keepalive interval; this much extra silence means the link is dead.
const KEEPALIVE_GRACE: Duration = Duration::from_secs(10);
const SEEN_CAPACITY: usize = 256;

/// Alert subscriptions: type, version and the scope it needs. Raids need none, so they cost 1 of the
/// `max_total_cost` of 10; the others are authorized by the broadcaster and cost 0.
const ALERT_SUBSCRIPTIONS: &[(&str, &str, Option<&str>)] = &[
    ("channel.follow", "2", Some("moderator:read:followers")),
    ("channel.subscribe", "1", Some("channel:read:subscriptions")),
    (
        "channel.subscription.message",
        "1",
        Some("channel:read:subscriptions"),
    ),
    (
        "channel.subscription.gift",
        "1",
        Some("channel:read:subscriptions"),
    ),
    ("channel.cheer", "1", Some("bits:read")),
    ("channel.raid", "1", None),
];

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Stable error codes, kept for the connection status shown in a later slice.
#[derive(Debug, thiserror::Error)]
pub enum EventSubError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error("eventsub.connect_failed")]
    Connect,
    #[error("eventsub.closed")]
    Closed,
    #[error("eventsub.silent_too_long")]
    Silent,
    #[error("eventsub.invalid_message")]
    InvalidMessage,
    #[error(transparent)]
    Helix(#[from] HelixError),
    #[error("eventsub.revoked")]
    Revoked,
}

pub struct EventSub {
    pub url: String,
    pub helix: Helix,
    pub tls: Arc<ClientConfig>,
    pub deliver: Arc<dyn Fn(ChatMessage) + Send + Sync>,
    pub deliver_alert: Arc<dyn Fn(Alert) + Send + Sync>,
}

impl EventSub {
    /// Runs until the task is dropped: connects while signed in, stops on logout, retries with backoff.
    pub async fn run(self) {
        let mut account = self.helix.auth().account();
        let mut failures = 0u32;
        loop {
            let user_id = account.borrow_and_update().clone();
            let Some(user_id) = user_id else {
                if account.changed().await.is_err() {
                    return;
                }
                continue;
            };
            let error = tokio::select! {
                result = self.session(&user_id, &mut failures) => match result {
                    Err(e) => e,
                },
                changed = account.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    continue;
                }
            };
            failures += 1;
            let backoff = crate::backoff(failures);
            // A 429 with a reset time waits at least that long (Helix points bucket).
            let wait = match &error {
                EventSubError::Helix(HelixError::RateLimited(Some(reset))) => backoff.max(*reset),
                _ => backoff,
            };
            tracing::warn!(error = %error, retry_in = ?wait, "EventSub session ended");
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                changed = account.changed() => if changed.is_err() { return },
            }
        }
    }

    async fn session(
        &self,
        user_id: &str,
        failures: &mut u32,
    ) -> Result<Infallible, EventSubError> {
        let mut socket = connect(&self.url, &self.tls).await?;
        // Until the welcome names the real interval; Twitch sends the welcome right after connecting.
        let mut keepalive = Duration::from_secs(10);
        let mut seen = VecDeque::new();
        loop {
            let message = receive(&mut socket, keepalive).await?;
            match message.metadata.message_type.as_str() {
                "session_welcome" => {
                    let session = message
                        .payload
                        .session
                        .ok_or(EventSubError::InvalidMessage)?;
                    keepalive = session.keepalive()?;
                    self.subscribe(user_id, &session.id).await?;
                    *failures = 0;
                }
                "session_reconnect" => {
                    let url = message
                        .payload
                        .session
                        .and_then(|s| s.reconnect_url)
                        .ok_or(EventSubError::InvalidMessage)?;
                    let mut next = connect(&url, &self.tls).await?;
                    // Subscriptions carry over; keep reading the old socket until the new one says welcome.
                    let welcome = self
                        .move_session(&mut socket, &mut next, keepalive, &mut seen)
                        .await?;
                    keepalive = welcome.keepalive()?;
                    socket = next;
                }
                "notification" => self.notify(message, &mut seen),
                "revocation" => return Err(EventSubError::Revoked),
                _ => {} // session_keepalive
            }
        }
    }

    async fn move_session(
        &self,
        old: &mut Socket,
        next: &mut Socket,
        keepalive: Duration,
        seen: &mut VecDeque<String>,
    ) -> Result<Session, EventSubError> {
        let mut old_open = true;
        loop {
            tokio::select! {
                result = receive(next, keepalive) => {
                    let message = result?;
                    return match message.metadata.message_type.as_str() {
                        "session_welcome" => message.payload.session.ok_or(EventSubError::InvalidMessage),
                        _ => Err(EventSubError::InvalidMessage),
                    };
                }
                result = receive(old, keepalive), if old_open => match result {
                    Ok(message) if message.metadata.message_type == "notification" => self.notify(message, seen),
                    Ok(_) => {}
                    Err(_) => old_open = false,
                },
            }
        }
    }

    fn notify(&self, message: Message, seen: &mut VecDeque<String>) {
        let id = message.metadata.message_id;
        // Twitch may deliver a notification more than once.
        if id.is_empty() || seen.contains(&id) {
            return;
        }
        if seen.len() == SEEN_CAPACITY {
            seen.pop_front();
        }
        seen.push_back(id.clone());
        let (Some(kind), Some(event)) = (message.metadata.subscription_type, message.payload.event)
        else {
            return;
        };
        let sent_at = message.metadata.message_timestamp;
        if kind == "channel.chat.message" {
            match serde_json::from_value::<ChatEvent>(event) {
                Ok(chat) => (self.deliver)(chat.into_message(sent_at)),
                Err(e) => tracing::warn!(error = %e, "unreadable chat notification"),
            }
            return;
        }
        match serde_json::from_value::<AlertEvent>(event)
            .ok()
            .and_then(|e| e.into_alert(&kind, id, sent_at))
        {
            Some(alert) => (self.deliver_alert)(alert),
            None => tracing::warn!(subscription = %kind, "unreadable alert notification"),
        }
    }

    /// One session subscribes once; a reconnect URL keeps its subscriptions and does not count as a new connection.
    async fn subscribe(&self, user_id: &str, session_id: &str) -> Result<(), EventSubError> {
        let chat = serde_json::json!({ "broadcaster_user_id": user_id, "user_id": user_id });
        self.create("channel.chat.message", "1", chat, session_id)
            .await?;
        let granted = self
            .helix
            .auth()
            .token()
            .map(|t| t.scopes)
            .unwrap_or_default();
        for &(kind, version, scope) in ALERT_SUBSCRIPTIONS {
            // A token from before the scope was added; the admin UI asks to sign in again.
            if scope.is_some_and(|s| !granted.iter().any(|g| g == s)) {
                continue;
            }
            let condition = match kind {
                "channel.follow" => {
                    serde_json::json!({ "broadcaster_user_id": user_id, "moderator_user_id": user_id })
                }
                "channel.raid" => serde_json::json!({ "to_broadcaster_user_id": user_id }),
                _ => serde_json::json!({ "broadcaster_user_id": user_id }),
            };
            match self.create(kind, version, condition, session_id).await {
                // E.g. a channel Twitch does not offer it to, or an EventSub limit (a 429); chat and the other
                // alerts carry on. Sign-in failures still end the session.
                Err(EventSubError::Helix(
                    e @ (HelixError::Refused(_) | HelixError::RateLimited(_)),
                )) => {
                    tracing::warn!(subscription = kind, error = ?e, "EventSub subscription refused");
                }
                other => other?,
            }
        }
        Ok(())
    }

    async fn create(
        &self,
        kind: &str,
        version: &str,
        condition: serde_json::Value,
        session_id: &str,
    ) -> Result<(), EventSubError> {
        let body = serde_json::json!({
            "type": kind,
            "version": version,
            "condition": condition,
            "transport": { "method": "websocket", "session_id": session_id },
        });
        Ok(self
            .helix
            .post_json("/eventsub/subscriptions", &body)
            .await?)
    }
}

/// `wss://` only, except `ws://` to this machine for fake services in tests; a reconnect never leaves TLS.
fn allowed_endpoint(url: &str) -> bool {
    let Ok(uri) = url.parse::<http::Uri>() else {
        return false;
    };
    match uri.scheme_str() {
        Some("wss") => true,
        Some("ws") => uri.host() == Some("127.0.0.1"),
        _ => false,
    }
}

async fn connect(url: &str, tls: &Arc<ClientConfig>) -> Result<Socket, EventSubError> {
    if !allowed_endpoint(url) {
        return Err(EventSubError::Connect);
    }
    let connecting = tokio_tungstenite::connect_async_tls_with_config(
        url,
        None,
        false,
        Some(Connector::Rustls(tls.clone())),
    );
    match tokio::time::timeout(CONNECT_TIMEOUT, connecting).await {
        Ok(Ok((socket, _))) => Ok(socket),
        _ => Err(EventSubError::Connect),
    }
}

/// Next JSON message; tungstenite answers pings itself while reading.
async fn receive(socket: &mut Socket, keepalive: Duration) -> Result<Message, EventSubError> {
    let read = async {
        loop {
            match socket.next().await {
                Some(Ok(tungstenite::Message::Text(text))) => {
                    return serde_json::from_str(text.as_str())
                        .map_err(|_| EventSubError::InvalidMessage);
                }
                Some(Ok(tungstenite::Message::Close(_))) | None | Some(Err(_)) => {
                    return Err(EventSubError::Closed);
                }
                Some(Ok(_)) => {}
            }
        }
    };
    tokio::time::timeout(keepalive + KEEPALIVE_GRACE, read)
        .await
        .map_err(|_| EventSubError::Silent)?
}

#[derive(Deserialize)]
struct Message {
    metadata: Metadata,
    payload: Payload,
}

#[derive(Deserialize)]
struct Metadata {
    message_type: String,
    #[serde(default)]
    message_id: String,
    #[serde(default)]
    message_timestamp: String,
    #[serde(default)]
    subscription_type: Option<String>,
}

#[derive(Deserialize)]
struct Payload {
    #[serde(default)]
    session: Option<Session>,
    #[serde(default)]
    event: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct Session {
    id: String,
    #[serde(default)]
    keepalive_timeout_seconds: Option<u64>,
    #[serde(default)]
    reconnect_url: Option<String>,
}

impl Session {
    fn keepalive(&self) -> Result<Duration, EventSubError> {
        self.keepalive_timeout_seconds
            .filter(|s| (1..=600).contains(s))
            .map(Duration::from_secs)
            .ok_or(EventSubError::InvalidMessage)
    }
}

#[derive(Deserialize)]
struct ChatEvent {
    message_id: String,
    chatter_user_id: String,
    chatter_user_login: String,
    chatter_user_name: String,
    #[serde(default)]
    color: String,
    message: ChatBody,
}

#[derive(Deserialize)]
struct ChatBody {
    text: String,
    #[serde(default)]
    fragments: Vec<ChatFragment>,
}

#[derive(Deserialize)]
struct ChatFragment {
    #[serde(rename = "type")]
    kind: String,
    text: String,
    #[serde(default)]
    emote: Option<EmoteRef>,
}

#[derive(Deserialize)]
struct EmoteRef {
    id: String,
}

impl ChatEvent {
    fn into_message(self, sent_at: String) -> ChatMessage {
        let valid_color = self.color.len() == 7
            && self.color.starts_with('#')
            && self.color[1..].bytes().all(|b| b.is_ascii_hexdigit());
        // Mentions, cheermotes and unknown types render as plain text.
        let fragments = self
            .message
            .fragments
            .into_iter()
            .map(|f| match (f.kind.as_str(), f.emote) {
                ("emote", Some(e)) => Fragment::Emote {
                    id: e.id,
                    text: f.text,
                },
                _ => Fragment::Text(f.text),
            })
            .collect();
        ChatMessage {
            id: self.message_id,
            user_id: self.chatter_user_id,
            user_login: self.chatter_user_login,
            user_name: self.chatter_user_name,
            color: valid_color.then_some(self.color),
            text: self.message.text,
            fragments,
            sent_at,
        }
    }
}

/// The fields of the six alert notifications this adapter reads; each type has only some of them.
#[derive(Deserialize)]
struct AlertEvent {
    #[serde(default)]
    user_name: Option<String>,
    #[serde(default)]
    from_broadcaster_user_name: Option<String>,
    #[serde(default)]
    is_anonymous: bool,
    #[serde(default)]
    tier: Option<String>,
    #[serde(default)]
    is_gift: bool,
    #[serde(default)]
    total: Option<u32>,
    #[serde(default)]
    cumulative_months: Option<u32>,
    #[serde(default)]
    message: Option<AlertText>,
    #[serde(default)]
    bits: Option<u32>,
    #[serde(default)]
    viewers: Option<u32>,
}

/// A cheer message is a string; a resub message is an object with its text and emotes.
#[derive(Deserialize)]
#[serde(untagged)]
enum AlertText {
    Plain(String),
    Rich { text: String },
}

impl AlertEvent {
    /// `None` when a field the type needs is missing.
    fn into_alert(self, subscription: &str, id: String, occurred_at: String) -> Option<Alert> {
        let tier = match self.tier.as_deref() {
            Some("2000") => 2,
            Some("3000") => 3,
            _ => 1,
        };
        let message = match self.message {
            Some(AlertText::Plain(text) | AlertText::Rich { text }) => text,
            None => String::new(),
        };
        let kind = match subscription {
            "channel.follow" => AlertKind::Follow,
            "channel.subscribe" => AlertKind::Subscribe {
                tier,
                gifted: self.is_gift,
            },
            "channel.subscription.message" => AlertKind::Resubscribe {
                tier,
                months: self.cumulative_months?,
                message,
            },
            "channel.subscription.gift" => AlertKind::Gift {
                tier,
                count: self.total?,
            },
            "channel.cheer" => AlertKind::Cheer {
                bits: self.bits?,
                message,
            },
            "channel.raid" => AlertKind::Raid {
                viewers: self.viewers?,
            },
            _ => return None,
        };
        let user_name = match self.is_anonymous {
            true => None,
            false => self.from_broadcaster_user_name.or(self.user_name),
        };
        Some(Alert {
            id,
            user_name,
            kind,
            occurred_at,
        })
    }
}

#[cfg(test)]
mod tests {
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
        response::Response,
        routing::{any, post},
    };
    use serde_json::{Value, json};
    use std::sync::Mutex;
    use tokio::sync::mpsc;

    /// Client-Id header and JSON body of one subscription request.
    type Subscription = (Option<String>, Value);

    #[derive(Clone, Default)]
    struct Fake {
        base: Arc<Mutex<String>>,
        subscriptions: Arc<Mutex<Vec<Subscription>>>,
    }

    fn welcome(id: &str) -> String {
        json!({"metadata":{"message_type":"session_welcome","message_id":format!("w-{id}")},
               "payload":{"session":{"id":id,"keepalive_timeout_seconds":10}}})
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

    async fn second(ws: WebSocketUpgrade) -> Response {
        ws.on_upgrade(|mut socket| async move {
            send(&mut socket, welcome("s2")).await;
            send(&mut socket, chat("n2", "c2", "red; background:url(x)")).await;
            for n in alert_notifications() {
                send(&mut socket, n).await;
            }
            hold_open(socket).await;
        })
    }

    async fn subscribe(
        State(f): State<Fake>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> StatusCode {
        let client_id = headers
            .get("Client-Id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let status = match body["type"].as_str() {
            Some("channel.follow") => StatusCode::FORBIDDEN,
            // An EventSub limit: a 429 without Ratelimit-Reset.
            Some("channel.subscription.gift") => StatusCode::TOO_MANY_REQUESTS,
            _ => StatusCode::ACCEPTED,
        };
        f.subscriptions.lock().unwrap().push((client_id, body));
        status
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
    fn signed_in_auth(http: HttpsClient) -> TwitchAuth {
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

    #[tokio::test]
    async fn delivers_chat_and_alerts_once_and_follows_reconnect() {
        let (base, fake) = fake_twitch().await;
        let config = tls::client_config().unwrap();
        let http = tls::https_client(config.clone());
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (alert_tx, mut alerts) = mpsc::unbounded_channel();
        let task = tokio::spawn(
            EventSub {
                url: format!("{}/ws", base.replace("http://", "ws://")),
                helix: Helix::new(&format!("{base}/helix"), signed_in_auth(http.clone()), http),
                tls: config,
                deliver: Arc::new(move |m| tx.send(m).unwrap()),
                deliver_alert: Arc::new(move |a| alert_tx.send(a).unwrap()),
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
            assert_eq!(alert.kind, kind, "{id}");
            assert_eq!(alert.occurred_at, "2026-11-16T10:00:00Z");
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(200), alerts.recv())
                .await
                .is_err(),
            "the broken cheer is not delivered"
        );

        let subs = fake.subscriptions.lock().unwrap().clone();
        let types: Vec<&str> = subs
            .iter()
            .map(|(_, b)| b["type"].as_str().unwrap())
            .collect();
        // Subscriptions carry over a reconnect; cheer needs bits:read, which this token lacks; the refused
        // follow and the rate-limited gift did not stop the rest.
        assert_eq!(
            types,
            [
                "channel.chat.message",
                "channel.follow",
                "channel.subscribe",
                "channel.subscription.message",
                "channel.subscription.gift",
                "channel.raid"
            ]
        );
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

    #[test]
    fn only_tls_or_loopback_endpoints() {
        assert!(allowed_endpoint(EVENTSUB_URL));
        assert!(allowed_endpoint("ws://127.0.0.1:1234/ws"));
        assert!(!allowed_endpoint("ws://eventsub.wss.twitch.tv/ws"));
        assert!(!allowed_endpoint("ws://127.0.0.1@evil.example/ws"));
        assert!(!allowed_endpoint("https://eventsub.wss.twitch.tv/ws"));
    }
}
