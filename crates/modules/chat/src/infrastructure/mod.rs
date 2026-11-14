//! Maps the internal chat message to the external `chat.message` payload.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use toolkit_core::events::{ChatMessage, Fragment, Source};
use utoipa::ToSchema;

pub(crate) const CHAT_MESSAGE: &str = "chat.message";
pub(crate) const CHAT_MESSAGE_VERSION: u32 = 1;

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

pub(crate) fn payload(m: ChatMessage) -> Value {
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
    })
    .expect("chat payload serializes")
}
