//! Twitch adapter. Types are hand-written serde structs covering only the fields we use.

pub mod auth;
pub mod eventsub;
pub mod helix;
pub mod streams;

/// Scopes requested at login. A stored token missing any of them (issued before one was added) asks the user
/// to sign in again.
pub const MVP_SCOPES: &[&str] = &[
    "user:read:chat",
    "moderator:read:followers",
    "channel:read:subscriptions",
    "channel:read:redemptions",
    "bits:read",
];
