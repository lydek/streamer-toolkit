//! Chat display (thin module): forwards chat messages to overlays while the module is enabled.

pub mod application;
mod domain;
mod infrastructure;

pub use application::ChatService;
pub use infrastructure::{ChatFragment, ChatMessageEvent, ChatMessagePayload, ChatMessageType};
