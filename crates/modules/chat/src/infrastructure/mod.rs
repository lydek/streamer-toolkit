//! Publishes chat messages on the overlay bus, mapped to the external `chat.message` payload.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use toolkit_core::{
    bus::{Event, EventBus},
    events::{ChatMessage, Fragment, Source},
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
            sent_at: "2026-11-13T10:00:00Z".into(),
        };

        chat.receive(message("a"), Source::Twitch);
        modules.set_enabled(modules::CHAT, false).unwrap();
        chat.receive(message("b"), Source::Twitch);

        let (snapshot, _) = bus.subscribe();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
        let recent = snapshot["payload"]["chat.message"].as_array().unwrap();
        assert_eq!(
            recent.len(),
            1,
            "the message sent while disabled is dropped"
        );
        assert_eq!(recent[0]["source"], "twitch");
        assert_eq!(recent[0]["payload"]["fragments"][1]["emoteId"], "25");
        // The OpenAPI type must describe the wire format exactly: no missing, extra or renamed fields.
        let typed: crate::ChatMessageEvent = serde_json::from_value(recent[0].clone()).unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap(), recent[0]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
