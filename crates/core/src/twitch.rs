//! What several modules need from Twitch without depending on the integration: looking up a viewer's profile
//! image (the Host implements it with Helix), and which image URLs may be shown.

use std::{collections::HashMap, future::Future, pin::Pin, time::Duration};

pub type ProfileBatch = Result<HashMap<String, String>, Duration>;

/// Looks up a viewer's Twitch profile image URL; the Host implements it with Helix. `None` when it cannot.
pub trait ProfileImages: Send + Sync + 'static {
    fn profile_image<'a>(
        &'a self,
        twitch_user_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Option<String>> + Send + 'a>>;

    /// Looks up a group of viewers. An error gives the delay before a retry.
    fn profile_images<'a>(
        &'a self,
        ids: &'a [String],
    ) -> Pin<Box<dyn Future<Output = ProfileBatch> + Send + 'a>> {
        Box::pin(async move {
            let mut images = HashMap::new();
            for id in ids {
                if let Some(url) = self.profile_image(id).await {
                    images.insert(id.clone(), url);
                }
            }
            Ok(images)
        })
    }
}

/// Only Twitch's image CDN is shown on overlays and stored.
pub fn is_twitch_image(url: &str) -> bool {
    url.len() <= 512
        && url.starts_with("https://static-cdn.jtvnw.net/")
        && !url.contains(|c: char| {
            c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '<' | '>' | '\\')
        })
}
