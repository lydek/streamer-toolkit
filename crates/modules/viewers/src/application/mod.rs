//! Records viewers one sighting at a time, in arrival order, with the time each arrived; avatars of viewers who have
//! none are looked up beside it, a few at a time, so lookups never hold up the records.

use std::{collections::HashSet, future::Future, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use tokio::sync::{Semaphore, mpsc};
use toolkit_core::{
    events::{Alert, ChannelPointsRedemption, ChatMessage, Source},
    twitch::{ProfileImages, is_twitch_image},
};

use crate::domain::Sighting;

const LOOKUP_TIMEOUT: Duration = Duration::from_secs(3);
/// Avatar lookups running at once.
const LOOKUPS_AT_ONCE: usize = 4;

#[derive(Debug, thiserror::Error)]
#[error("viewers.storage_failed: {0}")]
pub(crate) struct RepositoryError(pub(crate) String);

pub(crate) trait ViewerRepository: Send + Sync + 'static {
    /// Adds the viewer or updates their name, roles (when known) and last seen; `true` when they have no avatar.
    fn record(
        &self,
        s: &Sighting,
        at: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, RepositoryError>> + Send;
    fn set_avatar(
        &self,
        user_id: &str,
        url: &str,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
}

pub struct ViewerService {
    tx: mpsc::UnboundedSender<(Sighting, DateTime<Utc>)>,
}

impl ViewerService {
    /// Spawns the worker; needs a Tokio runtime.
    pub(crate) fn start<R: ViewerRepository>(repo: R, profiles: Arc<dyn ProfileImages>) -> Self {
        let repo = Arc::new(repo);
        let (tx, mut rx) = mpsc::unbounded_channel::<(Sighting, DateTime<Utc>)>();
        tokio::spawn(async move {
            // shortcut: one Helix lookup per viewer without an avatar, tried once per run; batch them (Get Users
            // takes 100 IDs) if a busy chat ever gets near the rate limit.
            let mut tried = HashSet::new();
            let lookups = Arc::new(Semaphore::new(LOOKUPS_AT_ONCE));
            while let Some((sighting, at)) = rx.recv().await {
                match repo.record(&sighting, at).await {
                    Ok(true) if tried.insert(sighting.user_id.clone()) => {
                        let (repo, profiles, lookups) =
                            (repo.clone(), profiles.clone(), lookups.clone());
                        tokio::spawn(async move {
                            let Ok(_turn) = lookups.acquire().await else {
                                return;
                            };
                            let found = tokio::time::timeout(
                                LOOKUP_TIMEOUT,
                                profiles.profile_image(&sighting.user_id),
                            )
                            .await
                            .ok()
                            .flatten()
                            .filter(|u| is_twitch_image(u));
                            if let Some(url) = found
                                && let Err(e) = repo.set_avatar(&sighting.user_id, &url).await
                            {
                                tracing::warn!(error = %e, "viewer avatar not stored");
                            }
                        });
                    }
                    Ok(_) => {}
                    Err(e) => tracing::error!(error = %e, "viewer not stored"),
                }
            }
        });
        Self { tx }
    }

    /// Simulated events never create or change viewers. The time is taken now, so a busy queue never makes a
    /// viewer look seen later than they were.
    fn see(&self, sighting: Sighting, source: Source) {
        if source == Source::Twitch {
            let _ = self.tx.send((sighting, Utc::now()));
        }
    }

    pub fn receive_chat(&self, m: &ChatMessage, source: Source) {
        self.see(Sighting::from_chat(m), source);
    }

    pub fn receive_alert(&self, a: &Alert, source: Source) {
        if let Some(sighting) = Sighting::from_alert(a) {
            self.see(sighting, source);
        }
    }

    pub fn receive_redemption(&self, r: &ChannelPointsRedemption, source: Source) {
        self.see(Sighting::from_redemption(r), source);
    }
}
