//! Receives chat messages from Twitch or the simulator and publishes them to the overlay bus.

use std::sync::Arc;

use toolkit_core::{
    bus::{Event, EventBus},
    events::{ChatMessage, Source},
    modules::{self, Modules},
};

use crate::infrastructure::{self, CHAT_MESSAGE, CHAT_MESSAGE_VERSION};

/// Recent chat kept in memory only, so a reloaded OBS source is not empty.
const RECENT: usize = 50;

pub struct ChatService {
    bus: Arc<EventBus>,
    modules: Arc<Modules>,
}

impl ChatService {
    pub fn new(bus: Arc<EventBus>, modules: Arc<Modules>) -> Self {
        bus.retain(CHAT_MESSAGE, RECENT);
        Self { bus, modules }
    }

    /// Dropped while the chat module is disabled.
    pub fn receive(&self, message: ChatMessage, source: Source) {
        if !self.modules.is_enabled(modules::CHAT) {
            return;
        }
        self.bus.publish(Event {
            id: message.id.clone(),
            event_type: CHAT_MESSAGE,
            version: CHAT_MESSAGE_VERSION,
            occurred_at: message.sent_at.clone(),
            source,
            payload: infrastructure::payload(message),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use toolkit_core::events::Fragment;

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
