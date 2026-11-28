//! Stores Twitch activity one record at a time, in arrival order, and deletes records past the retention on start
//! and once a day.

use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use tokio::sync::mpsc;
use toolkit_core::{
    events::{Alert, Source},
    settings::{self, RecordRetention},
};

use crate::domain::{self, Record};
use toolkit_core::time::Timestamp;

const PURGE_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, thiserror::Error)]
#[error("activity.storage_failed: {0}")]
pub(crate) struct RepositoryError(pub(crate) String);

pub(crate) trait ActivityRepository: Send + Sync + 'static {
    /// `false` when a record with that EventSub message ID is already stored.
    fn insert(&self, r: &Record) -> impl Future<Output = Result<bool, RepositoryError>> + Send;
    /// Deletes records from before `cutoff`; returns how many.
    fn delete_before(
        &self,
        cutoff: Timestamp,
    ) -> impl Future<Output = Result<u64, RepositoryError>> + Send;
    fn count_before(
        &self,
        cutoff: Timestamp,
    ) -> impl Future<Output = Result<u64, RepositoryError>> + Send;
    /// Deletes records from before `cutoff` (none for `None`) in a transaction that commits only if `save` then
    /// returns `true`; returns how many were deleted, or `None` when `save` failed and nothing was deleted.
    fn delete_before_if_saved(
        &self,
        cutoff: Option<Timestamp>,
        save: impl FnOnce() -> bool + Send,
    ) -> impl Future<Output = Result<Option<u64>, RepositoryError>> + Send;
}

/// Asks the stream module for the session an event arriving now belongs to; the answer comes once every stream
/// change before it is applied.
pub(crate) type SessionAsk = Arc<dyn Fn() -> SessionAnswer + Send + Sync>;
pub(crate) type SessionAnswer = Pin<Box<dyn Future<Output = Option<String>> + Send>>;

pub struct ActivityService {
    tx: mpsc::UnboundedSender<(Alert, SessionAnswer)>,
    ask: SessionAsk,
}

impl ActivityService {
    /// Spawns the worker that stores records in order, and the one that deletes old ones on start and once a
    /// day; needs a Tokio runtime.
    pub(crate) fn start<R: ActivityRepository>(
        repo: R,
        data_dir: PathBuf,
        ask: SessionAsk,
    ) -> Self {
        let repo = Arc::new(repo);
        let (tx, mut rx) = mpsc::unbounded_channel::<(
            Alert,
            Pin<Box<dyn Future<Output = Option<String>> + Send>>,
        )>();
        let worker = repo.clone();
        tokio::spawn(async move {
            while let Some((alert, session)) = rx.recv().await {
                let record = Record::from_alert(alert, session.await, Timestamp::now());
                if let Err(e) = worker.insert(&record).await {
                    tracing::error!(error = %e, "activity record not stored");
                }
            }
        });
        tokio::spawn(async move {
            loop {
                match retention(&data_dir) {
                    Some(r) => {
                        if let Err(e) = purge(&*repo, r).await {
                            tracing::error!(error = %e, "old activity records not deleted");
                        }
                    }
                    None => tracing::warn!(
                        code = "activity.retention_unreadable",
                        "settings.json unreadable: old activity records kept this time"
                    ),
                }
                tokio::time::sleep(PURGE_EVERY).await;
            }
        });
        Self { tx, ask }
    }

    /// Stores a follow, sub, resub, gift, cheer or raid from Twitch, in the session it arrived in; simulated ones are
    /// never stored.
    pub fn receive(&self, alert: Alert, source: Source) {
        if source == Source::Twitch {
            // Asked now, not when stored, so a start that arrived just before counts.
            let _ = self.tx.send((alert, (self.ask)()));
        }
    }
}

/// How many records a change to `next` would delete now.
pub(crate) async fn expired<R: ActivityRepository>(
    repo: &R,
    next: RecordRetention,
) -> Result<u64, RepositoryError> {
    match domain::cutoff(next, Timestamp::now()) {
        Some(cutoff) => repo.count_before(cutoff).await,
        None => Ok(0),
    }
}

/// Saves `next` and deletes what it no longer keeps (the broadcaster confirmed that) together: the delete commits
/// only once the retention is saved, so a failed save deletes nothing. Returns how many were deleted.
pub(crate) async fn set_retention<R: ActivityRepository>(
    repo: &R,
    data_dir: &Path,
    next: RecordRetention,
) -> Result<u64, SetError> {
    if !next.valid() {
        return Err(SetError::Invalid);
    }
    let save = || settings::update(data_dir, |s| s.record_retention = Some(next)).is_ok();
    match repo
        .delete_before_if_saved(domain::cutoff(next, Timestamp::now()), save)
        .await
    {
        Ok(Some(deleted)) => Ok(deleted),
        Ok(None) => Err(SetError::SaveFailed),
        // Saved, but the delete did not commit: the records stay until the next clean-up under the new retention.
        Err(e) => Err(SetError::Storage(e)),
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SetError {
    #[error("activity.invalid_retention")]
    Invalid,
    #[error("settings.save_failed")]
    SaveFailed,
    #[error(transparent)]
    Storage(RepositoryError),
}

/// The saved retention, the default when never changed; `None` when `settings.json` cannot be read or holds an
/// invalid one, so nothing is ever deleted on a guess.
pub(crate) fn retention(data_dir: &Path) -> Option<RecordRetention> {
    match settings::read(data_dir) {
        Ok(s) => match s.record_retention {
            None => Some(RecordRetention::DEFAULT),
            Some(r) => r.valid().then_some(r),
        },
        Err(_) => None,
    }
}

async fn purge<R: ActivityRepository>(
    repo: &R,
    retention: RecordRetention,
) -> Result<u64, RepositoryError> {
    match domain::cutoff(retention, Timestamp::now()) {
        Some(cutoff) => repo.delete_before(cutoff).await,
        None => Ok(0),
    }
}
