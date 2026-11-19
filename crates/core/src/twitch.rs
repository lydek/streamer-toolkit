//! What several modules need from Twitch without depending on the integration: looking up a viewer's profile
//! image (the Host implements it with Helix), and which image URLs may be shown.

use std::{future::Future, pin::Pin};

/// Looks up a viewer's Twitch profile image URL; the Host implements it with Helix. `None` when it cannot.
pub trait ProfileImages: Send + Sync + 'static {
    fn profile_image<'a>(
        &'a self,
        twitch_user_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + 'a>>;
}

/// Only Twitch's image CDN is shown on overlays and stored.
pub fn is_twitch_image(url: &str) -> bool {
    url.len() <= 512
        && url.starts_with("https://static-cdn.jtvnw.net/")
        && !url.contains(|c: char| {
            c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '<' | '>' | '\\')
        })
}
