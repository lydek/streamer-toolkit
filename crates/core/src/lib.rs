//! Shared core: settings, module registry, the external event envelope and the in-memory event bus.
//! Internal integration events (e.g. chat messages) live here too, so business modules never depend on
//! `integrations` or any external SDK.

pub mod audio;
pub mod bus;
pub mod commands;
pub mod events;
pub mod modules;
pub mod outbox;
pub mod presentation;
pub mod records;
pub mod settings;
pub mod streaming;
pub mod time;
pub mod twitch;

/// Default Host port; OBS Browser Source URLs depend on it staying stable.
pub const DEFAULT_PORT: u16 = 47810;
