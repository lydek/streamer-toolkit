//! EventSub over WebSocket: one session per signed-in account, reconnecting with backoff. Chat only for now.

use std::{collections::VecDeque, convert::Infallible, sync::Arc, time::Duration};

use futures_util::StreamExt;
use rustls::ClientConfig;
use serde::Deserialize;
use tokio::net::TcpStream;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream, tungstenite};
use toolkit_core::events::{ChatMessage, Fragment};

use super::{
    auth::AuthError,
    helix::{Helix, HelixError},
};

pub const EVENTSUB_URL: &str = "wss://eventsub.wss.twitch.tv/ws";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Twitch sends something at least every keepalive interval; this much extra silence means the link is dead.
const KEEPALIVE_GRACE: Duration = Duration::from_secs(10);
const MAX_BACKOFF_SECS: u64 = 60;
const SEEN_CAPACITY: usize = 256;

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
            let backoff =
                Duration::from_secs((1u64 << (failures - 1).min(6)).min(MAX_BACKOFF_SECS));
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
        seen.push_back(id);
        if message.metadata.subscription_type.as_deref() != Some("channel.chat.message") {
            return;
        }
        let Some(event) = message.payload.event else {
            return;
        };
        match serde_json::from_value::<ChatEvent>(event) {
            Ok(chat) => (self.deliver)(chat.into_message(message.metadata.message_timestamp)),
            Err(e) => tracing::warn!(error = %e, "unreadable chat notification"),
        }
    }

    /// One session subscribes once; a reconnect URL keeps its subscriptions and does not count as a new connection.
    async fn subscribe(&self, user_id: &str, session_id: &str) -> Result<(), EventSubError> {
        let body = serde_json::json!({
            "type": "channel.chat.message",
            "version": "1",
            "condition": { "broadcaster_user_id": user_id, "user_id": user_id },
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

    async fn second(ws: WebSocketUpgrade) -> Response {
        ws.on_upgrade(|mut socket| async move {
            send(&mut socket, welcome("s2")).await;
            send(&mut socket, chat("n2", "c2", "red; background:url(x)")).await;
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
        f.subscriptions.lock().unwrap().push((client_id, body));
        StatusCode::ACCEPTED
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

    fn signed_in_auth(http: HttpsClient) -> TwitchAuth {
        let store = Arc::new(MemoryStore::default());
        *store.value.lock().unwrap() = Some(
            json!({"access_token":"access-1","refresh_token":"refresh-1","user_id":"42",
                   "login":"streamer","scopes":["user:read:chat"]})
            .to_string(),
        );
        TwitchAuth::new("cid".into(), "http://127.0.0.1:9", http, store)
    }

    async fn next(rx: &mut mpsc::UnboundedReceiver<ChatMessage>) -> ChatMessage {
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn delivers_chat_once_and_follows_reconnect() {
        let (base, fake) = fake_twitch().await;
        let config = tls::client_config().unwrap();
        let http = tls::https_client(config.clone());
        let (tx, mut rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(
            EventSub {
                url: format!("{}/ws", base.replace("http://", "ws://")),
                helix: Helix::new(&format!("{base}/helix"), signed_in_auth(http.clone()), http),
                tls: config,
                deliver: Arc::new(move |m| tx.send(m).unwrap()),
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

        let subs = fake.subscriptions.lock().unwrap().clone();
        assert_eq!(subs.len(), 1, "subscriptions carry over a reconnect");
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
