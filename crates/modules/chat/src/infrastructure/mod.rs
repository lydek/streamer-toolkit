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

use crate::application::{ChatService, Removal};
use toolkit_core::time::{self, Timestamp};

pub mod db;
pub mod http;

/// Recent chat kept in memory only, so a reloaded OBS source is not empty.
const RECENT: usize = 50;
/// Recent removals kept for snapshots, so a window that reconnects drops what was taken down while it was away.
/// shortcut: a fixed count; a disconnect that misses more keeps the older ones until the page reloads.
const RECENT_REMOVALS: usize = 50;

const CHAT_MESSAGE: &str = "chat.message";
const CHAT_MESSAGE_VERSION: u32 = 1;
const CHAT_REMOVED: &str = "chat.removed";
const CHAT_REMOVED_VERSION: u32 = 1;
const CHAT_FIRST: &str = "chat.first";
const CHAT_FIRST_VERSION: u32 = 1;

/// The `payload` of a `chat.message` event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessagePayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broadcaster_id: Option<String>,
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
    /// The sender's first message in the channel, when known as it is published; a later `chat.first` says so for
    /// a message already shown.
    #[serde(default)]
    pub first_chat: bool,
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
    pub broadcaster_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// For a viewer or the whole chat: RFC 3339, the oldest message sent time it reaches (older ones stay).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
}

/// A `chat.removed` event exactly as `/ws/overlay` sends it: overlays and the admin window drop those messages
/// (the ones before it in `seq` order). The messages it names leave the snapshot, and the latest removals are kept
/// in it so a window that was away can drop what it still shows.
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

/// The `payload` of a `chat.first` event: the message already shown that is its sender's first in the channel.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatFirstPayload {
    pub message_id: String,
}

/// A `chat.first` event exactly as `/ws/overlay` sends it: the admin window marks that message 第一次聊天. Not kept
/// in snapshots; the message in the snapshot carries `firstChat` instead.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatFirstEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: ChatFirstType,
    pub version: u32,
    pub seq: u64,
    pub occurred_at: String,
    pub source: Source,
    pub payload: ChatFirstPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub enum ChatFirstType {
    #[serde(rename = "chat.first")]
    ChatFirst,
}

impl ChatService {
    /// Publishes accepted messages, removals and first-chat marks on `bus`; nothing is stored.
    pub fn new(bus: Arc<EventBus>, modules: Arc<Modules>) -> Self {
        bus.retain(CHAT_MESSAGE, RECENT);
        bus.retain(CHAT_REMOVED, RECENT_REMOVALS);
        let (removals, firsts) = (bus.clone(), bus.clone());
        Self::with_publisher(
            modules,
            move |message, source, first| {
                bus.publish(Event {
                    id: message.id.clone(),
                    event_type: CHAT_MESSAGE,
                    version: CHAT_MESSAGE_VERSION,
                    occurred_at: message.sent_at.clone(),
                    source,
                    payload: serde_json::to_value(ChatMessagePayload {
                        first_chat: first,
                        ..chat_payload(message)
                    })
                    .expect("chat payload serializes"),
                });
            },
            move |removal| publish_removal(&removals, removal),
            move |id| publish_first(&firsts, id),
        )
    }
}

fn publish_first(bus: &EventBus, id: &str) {
    bus.publish_changing(
        Event {
            id: uuid::Uuid::now_v7().to_string(),
            event_type: CHAT_FIRST,
            version: CHAT_FIRST_VERSION,
            occurred_at: time::millis(Timestamp::now()),
            source: Source::Twitch,
            payload: serde_json::to_value(ChatFirstPayload {
                message_id: id.to_owned(),
            })
            .expect("first chat serializes"),
        },
        CHAT_MESSAGE,
        |recent| {
            for e in recent.iter_mut().filter(|e| e.id == id) {
                e.payload["firstChat"] = serde_json::Value::Bool(true);
            }
        },
    );
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

fn publish_removal(
    bus: &EventBus,
    Removal {
        removal,
        since,
        channel,
    }: &Removal,
) {
    let (message_id, user_id) = match removal {
        ChatRemoval::Message { id } => (Some(id.clone()), None),
        ChatRemoval::User { user_id } => (None, Some(user_id.clone())),
        ChatRemoval::All => (None, None),
    };
    // Same boundary as storage; a message without a readable time counts as recent, as storage counts it.
    let reached = |e: &toolkit_core::bus::Envelope| {
        e.payload["broadcasterId"].as_str() == Some(channel.as_str())
            && since.is_none_or(|since| {
                e.occurred_at
                    .parse::<Timestamp>()
                    .map_or(true, |t| t >= since)
            })
    };
    let gone = |e: &toolkit_core::bus::Envelope| match removal {
        ChatRemoval::Message { id } => e.id == *id,
        ChatRemoval::User { user_id } => e.payload["userId"] == *user_id.as_str() && reached(e),
        ChatRemoval::All => reached(e),
    };
    bus.publish_dropping(
        Event {
            id: uuid::Uuid::now_v7().to_string(),
            event_type: CHAT_REMOVED,
            version: CHAT_REMOVED_VERSION,
            occurred_at: time::millis(Timestamp::now()),
            source: Source::Twitch,
            payload: serde_json::to_value(ChatRemovedPayload {
                broadcaster_id: since.map(|_| channel.clone()),
                message_id,
                user_id,
                since: since.map(time::micros),
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
    components(schemas(
        ChatMessageEvent,
        ChatRemovedEvent,
        ChatFirstEvent,
        http::StoredChatMessage
    ))
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
        broadcaster_id: Some(m.broadcaster_id),
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
        first_chat: false,
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
            broadcaster_id: "42".into(),
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
        let now = Timestamp::now().to_string();
        chat.receive(db::tests::message("m1", "7", "hi", &now), Source::Twitch);
        chat.receive(db::tests::message("m2", "8", "bye", &now), Source::Twitch);
        chat.receive(
            db::tests::message("sim", "9", "test", &now),
            Source::Simulation,
        );
        let (_, mut rx) = bus.subscribe();
        chat.remove("42".into(), ChatRemoval::Message { id: "m2".into() })
            .await;

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
        assert_eq!(
            snapshot["payload"]["chat.removed"][0], removed,
            "a window that was away learns of it"
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
        assert_eq!(db::session_chat_counts(&pool, "s1").await.unwrap().0, 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_first_chat_mark_before_or_after_its_message_marks_it_shown_and_stored() {
        let dir = std::env::temp_dir().join(format!("st-chat-first-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bus = Arc::new(EventBus::default());
        let pool = db::tests::pool().await;
        let chat = start(
            bus.clone(),
            Arc::new(Modules::load(dir.clone())),
            pool.clone(),
            || async { Some("s1".to_owned()) },
        );
        let now = Timestamp::now().to_string();
        let (_, mut rx) = bus.subscribe();
        let next = |rx: &mut tokio::sync::broadcast::Receiver<toolkit_core::bus::Published>| {
            serde_json::from_str::<serde_json::Value>(&rx.try_recv().unwrap()).unwrap()
        };

        // The mark comes first: the message is published already marked.
        chat.first_chat("early".into());
        chat.receive(db::tests::message("early", "7", "hi", &now), Source::Twitch);
        assert_eq!(next(&mut rx)["payload"]["firstChat"], true);

        // The message comes first: shown unmarked, then chat.first, and the snapshot has the mark.
        chat.receive(db::tests::message("late", "8", "yo", &now), Source::Twitch);
        assert_eq!(next(&mut rx)["payload"]["firstChat"], false);
        chat.first_chat("late".into());
        let first = next(&mut rx);
        assert_eq!(first["type"], "chat.first");
        assert_eq!(first["payload"], serde_json::json!({"messageId": "late"}));
        let typed: crate::ChatFirstEvent = serde_json::from_value(first.clone()).unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap(), first);
        let (snapshot, _) = bus.subscribe();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
        assert!(
            snapshot["payload"]["chat.message"]
                .as_array()
                .unwrap()
                .iter()
                .all(|e| e["payload"]["firstChat"] == true)
        );

        // A mark for a message never seen changes nothing.
        chat.first_chat("never".into());
        assert!(rx.try_recv().is_err());

        let count = loop {
            let n = db::session_chat_counts(&pool, "s1").await.unwrap().1;
            if n == 2 {
                break n;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        };
        assert_eq!(count, 2);
        assert!(
            db::recent_messages(&pool, 10)
                .await
                .unwrap()
                .iter()
                .all(|m| m.payload.first_chat)
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
