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
pub(crate) mod tests;
