//! The viewer directory (thin module): every Twitch user seen in chat, alerts or redemptions, with their name,
//! avatar, roles and when they were first and last seen; a renamed viewer is still the same row.

mod application;
mod domain;
mod infrastructure;

pub use application::ViewerService;
pub use infrastructure::{
    db::{MIGRATOR, ViewerName, avatars, names, search},
    start,
};
