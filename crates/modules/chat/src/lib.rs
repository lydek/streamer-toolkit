//! Chat (thin module): forwards chat messages to overlays while the module is enabled, stores the ones from Twitch
//! for the record retention, and clears what moderators take down.

pub mod application;
mod domain;
mod infrastructure;

pub use application::ChatService;
pub use infrastructure::{
    ChatMessageEvent, ChatMessagePayload, ChatRemovedEvent,
    db::{
        ChatRecords, MIGRATOR, StoredMessage, count_between, messages_between, recent_messages,
        session_message_count,
    },
    http::{StoredChatMessage, routes},
    openapi, start,
};
