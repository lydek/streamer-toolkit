//! The chat bot (complex module, docs/modules/chat-bot.md): commands answered as the streamer, through one
//! rate-limited queue; it never answers itself, and simulated events never make it post.

pub mod application;
mod domain;
mod infrastructure;

pub use application::{BotLookups, BotService, BoxFuture, ChatSender};
pub use infrastructure::{openapi, routes};
