//! Applies starts, ends and checks one at a time, in arrival order, and reports the live state after each.

use std::future::Future;

use chrono::{DateTime, Utc};
use tokio::sync::mpsc;
use toolkit_core::events::StreamChange;

use crate::domain::{self, Session, Step};

#[derive(Debug, thiserror::Error)]
#[error("stream.storage_failed: {0}")]
pub(crate) struct RepositoryError(pub(crate) String);

pub(crate) trait SessionRepository: Send + Sync + 'static {
    /// The session that started last, ended or not.
    fn latest(&self) -> impl Future<Output = Result<Option<Session>, RepositoryError>> + Send;
    /// Opens a session for `twitch_id`, or reopens the existing one, and ends any other session still open (its
    /// end was missed) at `started_at`; returns the session.
    fn open(
        &self,
        twitch_id: &str,
        started_at: DateTime<Utc>,
    ) -> impl Future<Output = Result<Session, RepositoryError>> + Send;
    fn end(
        &self,
        id: &str,
        at: DateTime<Utc>,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
}

/// The live state as the admin UI shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Broadcast {
    /// Nothing from Twitch yet (signed out, or the first check has not answered).
    Unknown,
    Offline,
    Live {
        started_at: DateTime<Utc>,
        viewers: Option<u32>,
    },
}

pub struct StreamService {
    tx: mpsc::UnboundedSender<StreamChange>,
}

impl StreamService {
    /// Spawns the worker; `report` gets the live state at start and after every change. Needs a Tokio runtime.
    pub(crate) fn start<R: SessionRepository>(
        repo: R,
        report: impl Fn(&Broadcast) + Send + Sync + 'static,
    ) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<StreamChange>();
        tokio::spawn(async move {
            report(&Broadcast::Unknown);
            let mut current = match repo.latest().await {
                Ok(latest) => latest,
                Err(e) => {
                    tracing::error!(error = %e, "stream sessions not read");
                    None
                }
            };
            let mut viewers = None;
            while let Some(change) = rx.recv().await {
                // Signed out: the state is unknown until Twitch says again; sessions stay as they are.
                if change == StreamChange::SignedOut {
                    report(&Broadcast::Unknown);
                    continue;
                }
                let now = Utc::now();
                let result = match domain::step(current.as_ref(), &change, now) {
                    Step::Open {
                        twitch_id,
                        started_at,
                    } => repo.open(&twitch_id, started_at).await.map(|s| {
                        viewers = None;
                        current = Some(s);
                    }),
                    Step::End => match &mut current {
                        Some(s) => repo.end(&s.id, now).await.map(|()| s.ended_at = Some(now)),
                        None => Ok(()),
                    },
                    Step::Keep => Ok(()),
                };
                if let Err(e) = result {
                    tracing::error!(error = %e, "stream session not stored");
                }
                let live = current.as_ref().filter(|s| s.live());
                if let (Some(s), StreamChange::Checked(Some(checked))) = (live, &change)
                    && s.twitch_id == checked.id
                {
                    viewers = Some(checked.viewers);
                }
                report(&match live {
                    Some(s) => Broadcast::Live {
                        started_at: s.started_at,
                        viewers,
                    },
                    None => Broadcast::Offline,
                });
            }
        });
        Self { tx }
    }

    /// Queues a start, an end or a check from Twitch.
    pub fn receive(&self, change: StreamChange) {
        let _ = self.tx.send(change);
    }
}
