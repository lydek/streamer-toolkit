//! Publishes chat messages on the overlay bus, mapped to the external `chat.message` payload.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use toolkit_core::{
    bus::{Event, EventBus},
    events::{ChatBadge, ChatMessage, Fragment, Source},
    modules::Modules,
};
use utoipa::ToSchema;

use crate::application::ChatService;

/// Recent chat kept in memory only, so a reloaded OBS source is not empty.
const RECENT: usize = 50;

const CHAT_MESSAGE: &str = "chat.message";
const CHAT_MESSAGE_VERSION: u32 = 1;

/// The `payload` of a `chat.message` event.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
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
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatReplyPayload {
    pub message_id: String,
    pub user_id: String,
    pub user_login: String,
    /// Raw viewer text; overlays escape it when rendering.
    pub user_name: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
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

impl ChatService {
    /// Publishes accepted messages on `bus`.
    pub fn new(bus: Arc<EventBus>, modules: Arc<Modules>) -> Self {
        bus.retain(CHAT_MESSAGE, RECENT);
        Self::with_publisher(modules, move |message, source| {
            bus.publish(Event {
                id: message.id.clone(),
                event_type: CHAT_MESSAGE,
                version: CHAT_MESSAGE_VERSION,
                occurred_at: message.sent_at.clone(),
                source,
                payload: payload(message),
            });
        })
    }
}

fn payload(m: ChatMessage) -> Value {
    let fragments = m
        .fragments
        .into_iter()
        .map(|f| match f {
            Fragment::Text(text) => ChatFragment::Text { text },
            Fragment::Emote { id, text } => ChatFragment::Emote { text, emote_id: id },
        })
        .collect();
    serde_json::to_value(ChatMessagePayload {
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
        reply: m.reply.map(|r| ChatReplyPayload {
            message_id: r.message_id,
            user_id: r.user_id,
            user_login: r.user_login,
            user_name: r.user_name,
        }),
    })
    .expect("chat payload serializes")
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
}
