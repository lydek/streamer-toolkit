//! Records viewers one sighting at a time, in arrival order, with the time each arrived; avatars of viewers who have
//! none are looked up beside it, a few at a time, so lookups never hold up the records.

use std::{collections::HashSet, future::Future, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use tokio::{
    sync::{mpsc, oneshot},
    time::{Instant, timeout_at},
};
use toolkit_core::{
    events::{Alert, ChannelPointsRedemption, ChatMessage, Source},
    twitch::{ProfileImages, is_twitch_image},
};

use crate::domain::Sighting;

const AVATAR_BATCH: usize = 100;
const AVATAR_WINDOW: Duration = Duration::from_millis(100);
const SIGHTING_BATCH: usize = 200;
const SIGHTING_WINDOW: Duration = Duration::from_millis(75);

#[derive(Debug, thiserror::Error)]
#[error("viewers.storage_failed: {0}")]
pub(crate) struct RepositoryError(pub(crate) String);

/// What recording a sighting found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Recorded {
    /// The viewer has no avatar yet.
    pub(crate) no_avatar: bool,
    /// The toolkit had never seen this viewer before.
    pub(crate) new: bool,
}

pub(crate) trait ViewerRepository: Send + Sync + 'static {
    /// Adds the viewer or updates their name, roles (when known) and last seen.
    fn record(
        &self,
        sightings: &[(Sighting, DateTime<Utc>)],
    ) -> impl Future<Output = Result<Vec<Recorded>, RepositoryError>> + Send;
    fn set_avatar(
        &self,
        user_id: &str,
        url: &str,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
}

/// A sighting, when it happened, and who wants to know whether it was the viewer's first.
type Job = (Sighting, DateTime<Utc>, Option<oneshot::Sender<bool>>);

pub struct ViewerService {
    tx: mpsc::UnboundedSender<Job>,
}

impl ViewerService {
    /// Spawns the worker; needs a Tokio runtime.
    pub(crate) fn start<R: ViewerRepository>(repo: R, profiles: Arc<dyn ProfileImages>) -> Self {
        let repo = Arc::new(repo);
        // shortcut: unbounded queues; a slow database or a rate-limited Helix lets them grow, so bound them (with
        // avatars scheduled from the database) if a busy channel ever shows memory growing.
        let (tx, mut rx) = mpsc::unbounded_channel::<Job>();
        let (avatars, mut requests) = mpsc::unbounded_channel::<String>();
        let avatar_repo = repo.clone();
        tokio::spawn(async move {
            while let Some(first) = requests.recv().await {
                let mut ids = vec![first];
                let deadline = Instant::now() + AVATAR_WINDOW;
                while ids.len() < AVATAR_BATCH {
                    match timeout_at(deadline, requests.recv()).await {
                        Ok(Some(id)) => ids.push(id),
                        _ => break,
                    }
                }
                for attempt in 0..3 {
                    match tokio::time::timeout(
                        Duration::from_secs(35),
                        profiles.profile_images(&ids),
                    )
                    .await
                    {
                        Ok(Ok(images)) => {
                            for id in &ids {
                                if let Some(url) = images.get(id).filter(|u| is_twitch_image(u))
                                    && let Err(e) = avatar_repo.set_avatar(id, url).await
                                {
                                    tracing::warn!(error = %e, "viewer avatar not stored");
                                }
                            }
                            break;
                        }
                        Ok(Err(wait)) if attempt < 2 => tokio::time::sleep(wait).await,
                        _ => break,
                    }
                }
            }
        });
        tokio::spawn(async move {
            let mut tried = HashSet::new();
            while let Some(first) = rx.recv().await {
                let mut jobs = vec![first];
                let deadline = Instant::now() + SIGHTING_WINDOW;
                while jobs.len() < SIGHTING_BATCH {
                    match timeout_at(deadline, rx.recv()).await {
                        Ok(Some(job)) => jobs.push(job),
                        _ => break,
                    }
                }
                let (sightings, answers): (Vec<_>, Vec<_>) = jobs
                    .into_iter()
                    .map(|(s, at, answer)| ((s, at), answer))
                    .unzip();
                match repo.record(&sightings).await {
                    Ok(results) => {
                        for (((sighting, _), answer), recorded) in
                            sightings.into_iter().zip(answers).zip(results)
                        {
                            if let Some(answer) = answer {
                                let _ = answer.send(recorded.new);
                            }
                            if recorded.no_avatar && tried.insert(sighting.user_id.clone()) {
                                let _ = avatars.send(sighting.user_id);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, count = sightings.len(), "viewer sightings not stored")
                    }
                }
            }
        });
        Self { tx }
    }

    /// Simulated events never create or change viewers. The time is taken now, so a busy queue never makes a
    /// viewer look seen later than they were.
    fn see(&self, sighting: Sighting, source: Source) {
        if source == Source::Twitch {
            let _ = self.tx.send((sighting, Utc::now(), None));
        }
    }

    pub fn receive_chat(&self, m: &ChatMessage, source: Source) {
        self.see(Sighting::from_chat(m), source);
    }

    /// Records a chat message's sender like [`Self::receive_chat`] and tells whether the toolkit had never seen
    /// them before (the fallback for 第一次聊天 when Twitch's own mark cannot be read); `false` for simulations or
    /// when it could not be stored.
    pub fn receive_chat_first_time(
        &self,
        m: &ChatMessage,
        source: Source,
    ) -> impl Future<Output = bool> + Send + use<> {
        let (first, answer) = oneshot::channel();
        if source == Source::Twitch {
            let _ = self
                .tx
                .send((Sighting::from_chat(m), Utc::now(), Some(first)));
        }
        async move { answer.await.unwrap_or(false) }
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
