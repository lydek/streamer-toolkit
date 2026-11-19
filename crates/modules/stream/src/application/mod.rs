//! Applies starts, ends and checks one at a time, in arrival order, and reports the live state after each.

use std::future::Future;

use chrono::{DateTime, Utc};
use tokio::sync::{mpsc, oneshot};
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
    /// Keeps `viewers` as the session's peak when no check counted more.
    fn raise_peak(
        &self,
        id: &str,
        viewers: u32,
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

/// What the worker handles, in arrival order.
enum Message {
    Change(StreamChange),
    /// Asks for the current session's ID once everything before it has been applied.
    Session(oneshot::Sender<Option<String>>),
}

pub struct StreamService {
    tx: mpsc::UnboundedSender<Message>,
}

impl StreamService {
    /// Spawns the worker; `report` gets the live state at start and after every change. Needs a Tokio runtime.
    pub(crate) fn start<R: SessionRepository>(
        repo: R,
        report: impl Fn(&Broadcast) + Send + Sync + 'static,
    ) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
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
            while let Some(message) = rx.recv().await {
                let change = match message {
                    Message::Change(change) => change,
                    Message::Session(reply) => {
                        let _ = reply.send(current.as_ref().map(|s| s.id.clone()));
                        continue;
                    }
                };
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
                    if let Err(e) = repo.raise_peak(&s.id, checked.viewers).await {
                        tracing::error!(error = %e, "stream peak viewers not stored");
                    }
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

    /// The current session's ID (the live stream's, or after it ended the last one's until the next starts; `None`
    /// before the first stream) once every start, end and check received so far is applied. Ask when an event
    /// arrives, so a follow right after a start gets that stream's session; other modules tag records with it.
    pub fn session_after_pending(&self) -> impl Future<Output = Option<String>> + Send + use<> {
        let (reply, answer) = oneshot::channel();
        let _ = self.tx.send(Message::Session(reply));
        async move { answer.await.ok().flatten() }
    }

    /// Queues a start, an end or a check from Twitch.
    pub fn receive(&self, change: StreamChange) {
        let _ = self.tx.send(Message::Change(change));
    }
}
