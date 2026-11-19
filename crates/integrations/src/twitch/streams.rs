//! Checks whether the broadcaster is live (Helix Get Streams, no scope needed) once a minute while signed in: it
//! catches a stream that was already live when the toolkit started or while EventSub was down, and keeps the
//! viewer count current.

use std::{sync::Arc, time::Duration};

use serde::Deserialize;
use toolkit_core::events::{LiveStream, StreamChange};

use super::{
    auth::AuthError,
    helix::{Helix, HelixError},
};

const EVERY: Duration = Duration::from_secs(60);

pub struct StreamCheck {
    pub helix: Helix,
    pub deliver: Arc<dyn Fn(StreamChange) + Send + Sync>,
}

impl StreamCheck {
    /// Runs until the task is dropped: checks at once on sign-in, then every minute; while signed out it checks
    /// nothing and says so once.
    pub async fn run(self) {
        let mut account = self.helix.auth().account();
        loop {
            let user_id = account.borrow_and_update().clone();
            if let Some(user_id) = user_id {
                match check(&self.helix, &user_id).await {
                    Ok(live) => (self.deliver)(StreamChange::Checked(live)),
                    Err(e) => tracing::warn!(error = %e, "stream check failed"),
                }
                tokio::select! {
                    _ = tokio::time::sleep(EVERY) => continue,
                    changed = account.changed() => if changed.is_err() { return },
                }
            } else {
                (self.deliver)(StreamChange::SignedOut);
                if account.changed().await.is_err() {
                    return;
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct Streams {
    data: Vec<StreamRow>,
}

#[derive(Deserialize)]
struct StreamRow {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    started_at: String,
    viewer_count: u32,
}

/// The broadcaster's live stream, or `None` when not live.
async fn check(helix: &Helix, user_id: &str) -> Result<Option<LiveStream>, HelixError> {
    // Twitch user IDs are digits; anything else never reaches the query string.
    if user_id.is_empty() || !user_id.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(None);
    }
    let body = helix
        .get_json(&format!("/streams?user_id={user_id}&type=live"))
        .await?;
    // An unreadable answer is a failed check, never "not live".
    live(body).map_err(|_| AuthError::InvalidResponse.into())
}

fn live(body: serde_json::Value) -> Result<Option<LiveStream>, serde_json::Error> {
    let streams: Streams = serde_json::from_value(body)?;
    Ok(streams
        .data
        .into_iter()
        .find(|s| s.kind == "live")
        .map(|s| LiveStream {
            id: s.id,
            started_at: s.started_at,
            viewers: s.viewer_count,
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_the_live_stream_or_none() {
        let body = json!({"data": [{"id": "123", "user_id": "42", "type": "live", "viewer_count": 78,
                                    "started_at": "2026-11-19T10:00:00Z", "title": "hi"}],
                          "pagination": {}});
        assert_eq!(
            live(body).unwrap(),
            Some(LiveStream {
                id: "123".into(),
                started_at: "2026-11-19T10:00:00Z".into(),
                viewers: 78
            })
        );
        assert_eq!(live(json!({"data": [], "pagination": {}})).unwrap(), None);
        assert!(
            live(json!({"error": "nope"})).is_err(),
            "an unreadable answer is not \"not live\""
        );
    }
}
