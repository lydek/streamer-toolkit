//! Twitch adapter. Types are hand-written serde structs covering only the fields we use.

pub mod auth;
pub mod eventsub;
pub mod helix;

/// Scopes requested at first login so later MVP slices never force a re-login.
pub const MVP_SCOPES: &[&str] = &[
    "user:read:chat",
    "moderator:read:followers",
    "channel:read:subscriptions",
    "channel:read:redemptions",
];
