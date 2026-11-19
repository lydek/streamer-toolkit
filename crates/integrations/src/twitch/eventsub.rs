//! EventSub over WebSocket: one session per signed-in account, reconnecting with backoff. Chat, the follow,
//! subscription, cheer and raid alerts, and channel point redemptions.

use std::{collections::VecDeque, convert::Infallible, sync::Arc, time::Duration};

use futures_util::StreamExt;
use rustls::ClientConfig;
use serde::Deserialize;
use tokio::net::TcpStream;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream, tungstenite};
use toolkit_core::events::{
    Alert, AlertKind, ChannelPointsRedemption, ChatBadge, ChatMessage, ChatRemoval, ChatReply,
    Fragment, RedemptionChange, RedemptionStatus, SharedChannel, StreamChange, TwitchBadge,
};

use super::{
    auth::AuthError,
    helix::{Helix, HelixError},
};

pub const EVENTSUB_URL: &str = "wss://eventsub.wss.twitch.tv/ws";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Twitch sends something at least every keepalive interval; this much extra silence means the link is dead.
const KEEPALIVE_GRACE: Duration = Duration::from_secs(if cfg!(test) { 1 } else { 10 });
const SEEN_CAPACITY: usize = 256;

const REDEMPTION_ADD: &str = "channel.channel_points_custom_reward_redemption.add";
const REDEMPTION_UPDATE: &str = "channel.channel_points_custom_reward_redemption.update";
const STREAM_ONLINE: &str = "stream.online";
const CHAT_DELETE: &str = "channel.chat.message_delete";
const CHAT_CLEAR_USER: &str = "channel.chat.clear_user_messages";
const CHAT_CLEAR: &str = "channel.chat.clear";
const STREAM_OFFLINE: &str = "stream.offline";

/// Subscriptions besides chat: type, version and the scope it needs. Raids and the stream's start and end need
/// none, so they cost 1 each of the `max_total_cost` of 10; the others are authorized by the broadcaster and cost 0.
const OTHER_SUBSCRIPTIONS: &[(&str, &str, Option<&str>)] = &[
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
    (REDEMPTION_ADD, "1", Some("channel:read:redemptions")),
    (REDEMPTION_UPDATE, "1", Some("channel:read:redemptions")),
    (STREAM_ONLINE, "1", None),
    (STREAM_OFFLINE, "1", None),
    // The same scope as chat itself.
    (CHAT_DELETE, "1", Some("user:read:chat")),
    (CHAT_CLEAR_USER, "1", Some("user:read:chat")),
    (CHAT_CLEAR, "1", Some("user:read:chat")),
];

pub(super) type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

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

pub type RemovalDelivery = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

pub struct EventSub {
    pub url: String,
    pub helix: Helix,
    pub tls: Arc<ClientConfig>,
    pub deliver: Arc<dyn Fn(ChatMessage) + Send + Sync>,
    pub deliver_alert: Arc<dyn Fn(Alert) + Send + Sync>,
    pub deliver_redemption: Arc<dyn Fn(ChannelPointsRedemption) + Send + Sync>,
    pub deliver_stream: Arc<dyn Fn(StreamChange) + Send + Sync>,
    /// A message, a viewer's messages or the whole chat taken down by a moderator, with the channel (broadcaster
    /// user ID) it happened in.
    pub deliver_removal: Arc<dyn Fn(String, ChatRemoval) -> RemovalDelivery + Send + Sync>,
    /// `true` once a session is subscribed, `false` when it ends or the account signs out.
    pub connected: Arc<dyn Fn(bool) + Send + Sync>,
}

impl EventSub {
    /// Runs until the task is dropped: connects while signed in, stops on logout, retries with backoff.
    pub async fn run(self) {
        let mut account = self.helix.auth().account();
        let mut failures = 0u32;
        loop {
            let user_id = account.borrow_and_update().clone();
            let Some(user_id) = user_id else {
                (self.connected)(false);
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
                    (self.connected)(false);
                    if changed.is_err() {
                        return;
                    }
                    continue;
                }
            };
            (self.connected)(false);
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
                    (self.connected)(true);
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
                "notification" => self.notify(message, &mut seen).await,
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
                    Ok(message) if message.metadata.message_type == "notification" => self.notify(message, seen).await,
                    Ok(_) => {}
                    Err(_) => old_open = false,
                },
            }
        }
    }

    async fn notify(&self, message: Message, seen: &mut VecDeque<String>) {
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
        if matches!(kind.as_str(), CHAT_DELETE | CHAT_CLEAR_USER | CHAT_CLEAR) {
            let channel = event["broadcaster_user_id"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            match chat_removal(&kind, event) {
                Some(removal) => (self.deliver_removal)(channel, removal).await,
                None => tracing::warn!(subscription = %kind, "unreadable chat removal"),
            }
            return;
        }
        if kind == STREAM_ONLINE || kind == STREAM_OFFLINE {
            match stream_change(&kind, event) {
                Some(change) => (self.deliver_stream)(change),
                None => tracing::debug!(subscription = %kind, "stream notification skipped"),
            }
            return;
        }
        let change = match kind.as_str() {
            REDEMPTION_ADD => Some(RedemptionChange::Added),
            REDEMPTION_UPDATE => Some(RedemptionChange::Updated),
            _ => None,
        };
        if let Some(change) = change {
            match serde_json::from_value::<RedemptionEvent>(event) {
                Ok(r) => (self.deliver_redemption)(r.into_redemption(change)),
                Err(e) => tracing::warn!(error = %e, "unreadable redemption notification"),
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
        self.create("channel.chat.message", "1", chat.clone(), session_id)
            .await?;
        let granted = self
            .helix
            .auth()
            .token()
            .map(|t| t.scopes)
            .unwrap_or_default();
        for &(kind, version, scope) in OTHER_SUBSCRIPTIONS {
            // A token from before the scope was added; the admin UI asks to sign in again.
            if scope.is_some_and(|s| !granted.iter().any(|g| g == s)) {
                continue;
            }
            let condition = match kind {
                "channel.follow" => {
                    serde_json::json!({ "broadcaster_user_id": user_id, "moderator_user_id": user_id })
                }
                "channel.raid" => serde_json::json!({ "to_broadcaster_user_id": user_id }),
                CHAT_DELETE | CHAT_CLEAR_USER | CHAT_CLEAR => chat.clone(),
                _ => serde_json::json!({ "broadcaster_user_id": user_id }),
            };
            match self.create(kind, version, condition, session_id).await {
                // E.g. a channel Twitch does not offer it to (channel points need Affiliate or Partner), or an
                // EventSub limit (a 429); chat and the others carry on. Sign-in failures still end the session.
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

/// A chat removal notification; `None` when a field it needs is missing.
fn chat_removal(kind: &str, event: serde_json::Value) -> Option<ChatRemoval> {
    let field = |name: &str| event.get(name)?.as_str().map(str::to_owned);
    match kind {
        CHAT_DELETE => Some(ChatRemoval::Message {
            id: field("message_id")?,
        }),
        CHAT_CLEAR_USER => Some(ChatRemoval::User {
            user_id: field("target_user_id")?,
        }),
        CHAT_CLEAR => Some(ChatRemoval::All),
        _ => None,
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

pub(super) async fn connect(url: &str, tls: &Arc<ClientConfig>) -> Result<Socket, EventSubError> {
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
    #[serde(default)]
    broadcaster_user_id: String,
    message_id: String,
    chatter_user_id: String,
    chatter_user_login: String,
    chatter_user_name: String,
    #[serde(default)]
    color: String,
    message: ChatBody,
    #[serde(default)]
    badges: Vec<BadgeRef>,
    #[serde(default)]
    message_type: String,
    #[serde(default)]
    cheer: Option<CheerRef>,
    #[serde(default)]
    reply: Option<ReplyRef>,
    /// Set during a shared chat: the channel the message was sent in (this channel's own too).
    #[serde(default)]
    source_broadcaster_user_id: Option<String>,
    #[serde(default)]
    source_broadcaster_user_login: Option<String>,
    #[serde(default)]
    source_broadcaster_user_name: Option<String>,
}

#[derive(Deserialize)]
struct BadgeRef {
    set_id: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    info: String,
}

#[derive(Deserialize)]
struct CheerRef {
    bits: u32,
}

#[derive(Deserialize)]
struct ReplyRef {
    parent_message_id: String,
    parent_user_id: String,
    parent_user_login: String,
    parent_user_name: String,
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
        let twitch_badges = self
            .badges
            .iter()
            .map(|b| TwitchBadge {
                set_id: b.set_id.clone(),
                id: b.id.clone(),
                info: b.info.clone(),
            })
            .collect();
        let shared_from = match (
            self.source_broadcaster_user_id,
            self.source_broadcaster_user_login,
            self.source_broadcaster_user_name,
        ) {
            (Some(user_id), Some(login), Some(name)) if user_id != self.broadcaster_user_id => {
                Some(SharedChannel {
                    user_id,
                    login,
                    name,
                })
            }
            _ => None,
        };
        // Roles: badges for other things (Turbo, bits, predictions…) are left out.
        let mut badges: Vec<ChatBadge> = Vec::new();
        for badge in self.badges {
            let role = match badge.set_id.as_str() {
                "broadcaster" => ChatBadge::Broadcaster,
                "moderator" => ChatBadge::Moderator,
                "vip" => ChatBadge::Vip,
                "subscriber" | "founder" => ChatBadge::Subscriber,
                _ => continue,
            };
            if !badges.contains(&role) {
                badges.push(role);
            }
        }
        ChatMessage {
            id: self.message_id,
            broadcaster_id: self.broadcaster_user_id,
            user_id: self.chatter_user_id,
            user_login: self.chatter_user_login,
            user_name: self.chatter_user_name,
            color: valid_color.then_some(self.color),
            text: self.message.text,
            fragments,
            badges,
            highlighted: self.message_type == "channel_points_highlighted",
            bits: self.cheer.map_or(0, |c| c.bits),
            twitch_badges,
            shared_from,
            reply: self.reply.map(|r| ChatReply {
                message_id: r.parent_message_id,
                user_id: r.parent_user_id,
                user_login: r.parent_user_login,
                user_name: r.parent_user_name,
            }),
            sent_at,
        }
    }
}

/// The fields of the six alert notifications this adapter reads; each type has only some of them.
#[derive(Deserialize)]
struct AlertEvent {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    user_login: Option<String>,
    #[serde(default)]
    user_name: Option<String>,
    #[serde(default)]
    from_broadcaster_user_id: Option<String>,
    #[serde(default)]
    from_broadcaster_user_login: Option<String>,
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
        // A raid's viewer is the raiding channel.
        let (user_id, user_login, user_name) = match self.is_anonymous {
            true => (None, None, None),
            false => (
                self.from_broadcaster_user_id.or(self.user_id),
                self.from_broadcaster_user_login.or(self.user_login),
                self.from_broadcaster_user_name.or(self.user_name),
            ),
        };
        Some(Alert {
            id,
            user_id,
            user_login,
            user_name,
            kind,
            occurred_at,
        })
    }
}

#[derive(Deserialize)]
struct StreamOnline {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    started_at: String,
}

/// `None` for an unreadable event, or a start that is not a live stream (a rerun, a premiere, a watch party).
fn stream_change(subscription: &str, event: serde_json::Value) -> Option<StreamChange> {
    match subscription {
        STREAM_OFFLINE => Some(StreamChange::Offline),
        _ => {
            let online: StreamOnline = serde_json::from_value(event).ok()?;
            (online.kind == "live").then_some(StreamChange::Online {
                id: online.id,
                started_at: online.started_at,
            })
        }
    }
}

/// A custom reward redemption; `.add` and `.update` carry the same fields.
#[derive(Deserialize)]
struct RedemptionEvent {
    id: String,
    user_id: String,
    user_login: String,
    user_name: String,
    #[serde(default)]
    user_input: String,
    status: String,
    reward: RedemptionReward,
    redeemed_at: String,
}

#[derive(Deserialize)]
struct RedemptionReward {
    id: String,
    title: String,
    cost: u32,
}

impl RedemptionEvent {
    fn into_redemption(self, change: RedemptionChange) -> ChannelPointsRedemption {
        // EventSub statuses are lowercase; "cancelled" appears in Twitch's own docs, so both spellings count.
        let status = match self.status.as_str() {
            "unfulfilled" => RedemptionStatus::Unfulfilled,
            "fulfilled" => RedemptionStatus::Fulfilled,
            "canceled" | "cancelled" => RedemptionStatus::Canceled,
            _ => RedemptionStatus::Unknown,
        };
        ChannelPointsRedemption {
            id: self.id,
            change,
            user_id: self.user_id,
            user_login: self.user_login,
            user_name: self.user_name,
            reward_id: self.reward.id,
            reward_title: self.reward.title,
            reward_cost: self.reward.cost,
            user_input: self.user_input,
            status,
            redeemed_at: self.redeemed_at,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
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

    async fn second(ws: WebSocketUpgrade) -> Response {
        ws.on_upgrade(|mut socket| async move {
            send(&mut socket, welcome("s2")).await;
            send(&mut socket, chat("n2", "c2", "red; background:url(x)")).await;
            for n in alert_notifications()
                .into_iter()
                .chain(redemption_notifications())
            {
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
        let task = tokio::spawn(
            EventSub {
                url: format!("{}/ws", base.replace("http://", "ws://")),
                helix: Helix::new(&format!("{base}/helix"), signed_in_auth(http.clone()), http),
                tls: config,
                deliver: Arc::new(move |m| tx.send(m).unwrap()),
                deliver_alert: Arc::new(move |a| alert_tx.send(a).unwrap()),
                deliver_redemption: Arc::new(move |r| redemption_tx.send(r).unwrap()),
                deliver_stream: Arc::new(|_| {}),
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
    /// any, then silence. The chat subscription answers `chat_status`; the others are accepted.
    #[derive(Clone)]
    struct Scripted {
        after_welcome: Option<String>,
        chat_status: StatusCode,
        rate_limit_reset: Option<u64>,
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
        if body["type"] != "channel.chat.message" {
            return StatusCode::ACCEPTED.into_response();
        }
        let session = body["transport"]["session_id"].as_str().unwrap_or_default();
        f.chat_sessions.lock().unwrap().push(session.into());
        match f.rate_limit_reset {
            Some(reset) => {
                (f.chat_status, [("Ratelimit-Reset", reset.to_string())]).into_response()
            }
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
        let plain =
            chat_event(json!({"message_type":"text","badges":[],"cheer":null,"reply":null}));
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
        let plain =
            chat_event(json!({"broadcaster_user_id":"42","source_broadcaster_user_id":null}));
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
}
