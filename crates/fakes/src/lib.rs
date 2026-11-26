//! Fake OBS and Twitch services for developing and testing without either. They answer only what the app sends,
//! bind `127.0.0.1` only, and are never part of a release (no crate depends on this one).

pub mod control;
pub mod events;
pub mod metrics;
pub mod obs;
#[cfg(feature = "ui")]
pub mod panel;
pub mod twitch;
