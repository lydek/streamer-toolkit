//! Publishes chat messages on the overlay bus, mapped to the external `chat.message` payload, and what moderators
//! take down as `chat.removed`; stores messages in SQLite.

use std::{future::Future, sync::Arc};

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use toolkit_core::{
    bus::{Event, EventBus},
    events::{ChatBadge, ChatMessage, ChatRemoval, Fragment, Source},
    modules::Modules,
};
use utoipa::ToSchema;

use crate::application::ChatService;

pub mod db;
pub mod http;

/// Recent chat kept in memory only, so a reloaded OBS source is not empty.
const RECENT: usize = 50;

const CHAT_MESSAGE: &str = "chat.message";
const CHAT_MESSAGE_VERSION: u32 = 1;
const CHAT_REMOVED: &str = "chat.removed";
const CHAT_REMOVED_VERSION: u32 = 1;

/// The `payload` of a `chat.message` event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessagePayload {
    pub user_id: String,
    pub user_login: String,
    pub user_name: String,
    /// `#RRGGBB`, or `null` when the user never picked a color.
    #[schema(required)]
    pub color: Option<String>,
    /// Raw viewer text; overlays escape it when rendering.
    pub text: String,
    pub fragments: Vec<ChatFragment>,
    /// The roles the sender's badges show.
    pub badges: Vec<ChatBadgeName>,
    /// Highlighted with channel points.
    pub highlighted: bool,
    /// Bits cheered with this message, when there are any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bits: Option<u32>,
    /// The message this one replies to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply: Option<ChatReplyPayload>,
    /// Every badge as Twitch names it, in Twitch's order (the admin window shows their images).
    #[serde(default)]
    pub twitch_badges: Vec<TwitchBadgePayload>,
    /// During a shared chat, the other channel the message was sent in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_from: Option<SharedChannelPayload>,
}

/// A chat badge as Twitch names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TwitchBadgePayload {
    /// E.g. `subscriber`.
    pub set_id: String,
    /// The version within the set (e.g. a subscriber badge's tier and months).
    pub id: String,
    /// Extra info, e.g. the months subscribed; often empty.
    pub info: String,
}

/// Another channel in a shared chat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SharedChannelPayload {
    pub user_id: String,
    pub user_login: String,
    /// Raw text.
    pub user_name: String,
}

/// A role shown by a chat badge (a founder counts as a subscriber).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChatBadgeName {
    Broadcaster,
    Moderator,
    Vip,
    Subscriber,
}

/// The message a reply answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatReplyPayload {
    pub message_id: String,
    pub user_id: String,
    pub user_login: String,
    /// Raw viewer text; overlays escape it when rendering.
    pub user_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatFragment {
    Text {
        text: String,
    },
    Emote {
        text: String,
        #[serde(rename = "emoteId")]
        emote_id: String,
    },
}

/// A `chat.message` event exactly as `/ws/overlay` sends it: the event envelope around its payload.
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: ChatMessageType,
    pub version: u32,
    pub seq: u64,
    pub occurred_at: String,
    pub source: Source,
    pub payload: ChatMessagePayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub enum ChatMessageType {
    #[serde(rename = "chat.message")]
    ChatMessage,
}

/// The `payload` of a `chat.removed` event: a moderator took down one message (`messageId`), every recent message
/// of one viewer (`userId`), or the whole chat (neither).
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatRemovedPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

/// A `chat.removed` event exactly as `/ws/overlay` sends it: overlays and the admin window drop those messages. Not
/// kept in snapshots; the messages it names leave the snapshot instead.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatRemovedEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: ChatRemovedType,
    pub version: u32,
    pub seq: u64,
    pub occurred_at: String,
    pub source: Source,
    pub payload: ChatRemovedPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub enum ChatRemovedType {
    #[serde(rename = "chat.removed")]
    ChatRemoved,
}

impl ChatService {
    /// Publishes accepted messages and removals on `bus`; nothing is stored.
    pub fn new(bus: Arc<EventBus>, modules: Arc<Modules>) -> Self {
        bus.retain(CHAT_MESSAGE, RECENT);
        let removals = bus.clone();
        Self::with_publisher(
            modules,
            move |message, source| {
                bus.publish(Event {
                    id: message.id.clone(),
                    event_type: CHAT_MESSAGE,
                    version: CHAT_MESSAGE_VERSION,
                    occurred_at: message.sent_at.clone(),
                    source,
                    payload: serde_json::to_value(chat_payload(message))
                        .expect("chat payload serializes"),
                });
            },
            move |removal| publish_removal(&removals, removal),
        )
    }
}

/// Publishes and stores chat in `pool`, which must already have [`db::MIGRATOR`] applied; `session` asks for the
/// stream session a message arriving now belongs to. Needs a Tokio runtime.
pub fn start<F>(
    bus: Arc<EventBus>,
    modules: Arc<Modules>,
    pool: SqlitePool,
    session: impl Fn() -> F + Send + Sync + 'static,
) -> ChatService
where
    F: Future<Output = Option<String>> + Send + 'static,
{
    ChatService::new(bus, modules).storing(
        db::SqliteChat::new(pool),
        Arc::new(move || Box::pin(session())),
    )
}

fn publish_removal(bus: &EventBus, removal: &ChatRemoval) {
    let (message_id, user_id) = match removal {
        ChatRemoval::Message { id } => (Some(id.clone()), None),
        ChatRemoval::User { user_id } => (None, Some(user_id.clone())),
        ChatRemoval::All => (None, None),
    };
    let gone = |e: &toolkit_core::bus::Envelope| match removal {
        ChatRemoval::Message { id } => e.id == *id,
        ChatRemoval::User { user_id } => e.payload["userId"] == *user_id.as_str(),
        ChatRemoval::All => true,
    };
    bus.publish_dropping(
        Event {
            id: uuid::Uuid::now_v7().to_string(),
            event_type: CHAT_REMOVED,
            version: CHAT_REMOVED_VERSION,
            occurred_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            source: Source::Twitch,
            payload: serde_json::to_value(ChatRemovedPayload {
                message_id,
                user_id,
            })
            .expect("removal serializes"),
        },
        CHAT_MESSAGE,
        gone,
    );
}

/// Every chat type for the Host's OpenAPI document.
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(http::messages),
    components(schemas(ChatMessageEvent, ChatRemovedEvent, http::StoredChatMessage))
)]
struct ChatApi;

/// This module's part of the Host's OpenAPI document.
pub fn openapi() -> utoipa::openapi::OpenApi {
    <ChatApi as utoipa::OpenApi>::openapi()
}

pub(crate) fn chat_payload(m: ChatMessage) -> ChatMessagePayload {
    let fragments = m
        .fragments
        .into_iter()
        .map(|f| match f {
            Fragment::Text(text) => ChatFragment::Text { text },
            Fragment::Emote { id, text } => ChatFragment::Emote { text, emote_id: id },
        })
        .collect();
    ChatMessagePayload {
        user_id: m.user_id,
        user_login: m.user_login,
        user_name: m.user_name,
        color: m.color,
        text: m.text,
        fragments,
        badges: m
            .badges
            .into_iter()
            .map(|b| match b {
                ChatBadge::Broadcaster => ChatBadgeName::Broadcaster,
                ChatBadge::Moderator => ChatBadgeName::Moderator,
                ChatBadge::Vip => ChatBadgeName::Vip,
                ChatBadge::Subscriber => ChatBadgeName::Subscriber,
            })
            .collect(),
        highlighted: m.highlighted,
        bits: (m.bits > 0).then_some(m.bits),
        twitch_badges: m
            .twitch_badges
            .into_iter()
            .map(|b| TwitchBadgePayload {
                set_id: b.set_id,
                id: b.id,
                info: b.info,
            })
            .collect(),
        shared_from: m.shared_from.map(|c| SharedChannelPayload {
            user_id: c.user_id,
            user_login: c.login,
            user_name: c.name,
        }),
        reply: m.reply.map(|r| ChatReplyPayload {
            message_id: r.message_id,
            user_id: r.user_id,
            user_login: r.user_login,
            user_name: r.user_name,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use toolkit_core::modules;

    #[test]
    fn publishes_only_while_enabled() {
        let dir = std::env::temp_dir().join(format!("st-chat-{}", std::process::id()));
        let bus = Arc::new(EventBus::default());
        let modules = Arc::new(Modules::load(dir.clone()));
        let chat = ChatService::new(bus.clone(), modules.clone());
        let message = |id: &str| ChatMessage {
            id: id.into(),
            user_id: "7".into(),
            user_login: "viewer".into(),
            user_name: "Viewer".into(),
            color: Some("#1E90FF".into()),
            text: "hi Kappa".into(),
            fragments: vec![
                Fragment::Text("hi ".into()),
                Fragment::Emote {
                    id: "25".into(),
                    text: "Kappa".into(),
                },
            ],
            badges: vec![],
            highlighted: false,
            bits: 0,
            reply: None,
            twitch_badges: Vec::new(),
            shared_from: None,
            sent_at: "2026-11-13T10:00:00Z".into(),
        };

        chat.receive(message("a"), Source::Twitch);
        chat.receive(
            ChatMessage {
                badges: vec![ChatBadge::Moderator, ChatBadge::Subscriber],
                highlighted: true,
                bits: 100,
                reply: Some(toolkit_core::events::ChatReply {
                    message_id: "p".into(),
                    user_id: "8".into(),
                    user_login: "other".into(),
                    user_name: "Other".into(),
                }),
                ..message("full")
            },
            Source::Twitch,
        );
        modules.set_enabled(modules::CHAT, false).unwrap();
        chat.receive(message("b"), Source::Twitch);

        let (snapshot, _) = bus.subscribe();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
        let recent = snapshot["payload"]["chat.message"].as_array().unwrap();
        assert_eq!(
            recent.len(),
            2,
            "the message sent while disabled is dropped"
        );
        assert_eq!(recent[0]["source"], "twitch");
        assert_eq!(recent[0]["payload"]["fragments"][1]["emoteId"], "25");
        assert_eq!(recent[0]["payload"]["badges"], serde_json::json!([]));
        assert!(
            recent[0]["payload"].get("bits").is_none()
                && recent[0]["payload"].get("reply").is_none()
        );
        let full = &recent[1]["payload"];
        assert_eq!(
            full["badges"],
            serde_json::json!(["moderator", "subscriber"])
        );
        assert_eq!(full["highlighted"], true);
        assert_eq!(full["bits"], 100);
        assert_eq!(
            full["reply"],
            serde_json::json!({"messageId": "p", "userId": "8", "userLogin": "other", "userName": "Other"})
        );
        // The OpenAPI type must describe the wire format exactly: no missing, extra or renamed fields.
        for event in recent {
            let typed: crate::ChatMessageEvent = serde_json::from_value(event.clone()).unwrap();
            assert_eq!(&serde_json::to_value(typed).unwrap(), event);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn twitch_chat_is_stored_with_its_session_and_removals_reach_overlays_and_storage() {
        let dir = std::env::temp_dir().join(format!("st-chat-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bus = Arc::new(EventBus::default());
        let pool = db::tests::pool().await;
        let chat = start(
            bus.clone(),
            Arc::new(Modules::load(dir.clone())),
            pool.clone(),
            || async { Some("s1".to_owned()) },
        );
        let now = chrono::Utc::now().to_rfc3339();
        chat.receive(db::tests::message("m1", "7", "hi", &now), Source::Twitch);
        chat.receive(db::tests::message("m2", "8", "bye", &now), Source::Twitch);
        chat.receive(
            db::tests::message("sim", "9", "test", &now),
            Source::Simulation,
        );
        let (_, mut rx) = bus.subscribe();
        chat.remove(ChatRemoval::Message { id: "m2".into() });

        let removed: serde_json::Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
        assert_eq!(removed["type"], "chat.removed");
        assert_eq!(removed["payload"], serde_json::json!({"messageId": "m2"}));
        let typed: crate::ChatRemovedEvent = serde_json::from_value(removed.clone()).unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap(), removed);
        let (snapshot, _) = bus.subscribe();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
        let ids: Vec<&str> = snapshot["payload"]["chat.message"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            ["m1", "sim"],
            "a reconnecting overlay no longer gets it"
        );

        // The worker stores in order: the removal comes after both messages.
        let stored = loop {
            let stored = db::recent_messages(&pool, 10).await.unwrap();
            if stored.iter().any(|m| m.deleted) {
                break stored;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        };
        let mut rows: Vec<(&str, &str, bool)> = stored
            .iter()
            .map(|m| (m.id.as_str(), m.payload.text.as_str(), m.deleted))
            .collect();
        rows.sort();
        assert_eq!(
            rows,
            [("m1", "hi", false), ("m2", "", true)],
            "simulated chat is never stored"
        );
        assert_eq!(db::session_message_count(&pool, "s1").await.unwrap(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
