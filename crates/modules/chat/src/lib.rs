//! Chat (thin module): forwards chat messages to overlays while the module is enabled, stores the ones from Twitch
//! for the record retention, and clears what moderators take down.

pub mod application;
mod domain;
mod infrastructure;

pub use application::ChatService;
pub use infrastructure::{
    ChatMessageEvent, ChatRemovedEvent,
    db::{ChatRecords, MIGRATOR, StoredMessage, recent_messages, session_message_count},
    http::routes,
    openapi, start,
};
