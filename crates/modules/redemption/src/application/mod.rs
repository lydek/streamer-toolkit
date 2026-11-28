//! Records redemptions from Twitch: one record per Twitch redemption ID, status changes checked by the aggregate.
//! Events are handled one at a time, in arrival order.

use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use tokio::sync::mpsc;
use toolkit_core::{
    events::{ChannelPointsRedemption, RedemptionChange, RedemptionStatus, Source},
    modules::{self, Modules},
};

use crate::domain::{Redemption, Reward, Status, TwitchRedemptionId, Viewer};
use toolkit_core::time::Timestamp;

#[derive(Debug, thiserror::Error)]
#[error("redemption.storage_failed: {0}")]
pub(crate) struct RepositoryError(pub(crate) String);

/// Storage port; the SQLite implementation lives in `infrastructure::db`.
pub(crate) trait RedemptionRepository: Send + Sync + 'static {
    /// `false` when a record with the same Twitch redemption ID already exists; nothing is written then.
    fn insert(&self, r: &Redemption) -> impl Future<Output = Result<bool, RepositoryError>> + Send;
    fn find_by_twitch_id(
        &self,
        id: &TwitchRedemptionId,
    ) -> impl Future<Output = Result<Option<Redemption>, RepositoryError>> + Send;
    fn save_status(
        &self,
        r: &Redemption,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
}

/// Waits before each retry of a failed write (e.g. the database busy); the queue waits too, so order holds.
const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
];

pub struct RedemptionService {
    tx: mpsc::UnboundedSender<ChannelPointsRedemption>,
    modules: Arc<Modules>,
    failed: Arc<AtomicU64>,
}

impl RedemptionService {
    /// Spawns the worker that stores events in order and calls `changed` after a record is created or changes
    /// status; needs a Tokio runtime.
    pub(crate) fn start<R, F>(repo: R, modules: Arc<Modules>, changed: F) -> Self
    where
        R: RedemptionRepository,
        F: Fn(&Redemption) + Send + 'static,
    {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let failed = Arc::new(AtomicU64::new(0));
        let failures = failed.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                match record_retrying(&repo, event).await {
                    Ok(Some(r)) => changed(&r),
                    Ok(None) => {}
                    Err(e) => {
                        failures.fetch_add(1, Ordering::Relaxed);
                        tracing::error!(code = "redemption.storage_failed", error = %e, "redemption not stored")
                    }
                }
            }
        });
        Self {
            tx,
            modules,
            failed,
        }
    }

    /// Redemptions not stored since the app started, even after retries.
    pub fn failed_writes(&self) -> u64 {
        self.failed.load(Ordering::Relaxed)
    }

    /// Simulated redemptions never go into the records. While the module is disabled, redemptions are dropped
    /// like other in-memory events.
    pub fn receive(&self, redemption: ChannelPointsRedemption, source: Source) {
        if source == Source::Simulation || !self.modules.is_enabled(modules::REDEMPTION) {
            return;
        }
        let _ = self.tx.send(redemption);
    }
}

/// [`record`], retried after each of [`RETRY_DELAYS`]; a write that went through is never repeated (the
/// redemption ID keeps one record).
async fn record_retrying<R: RedemptionRepository>(
    repo: &R,
    event: ChannelPointsRedemption,
) -> Result<Option<Redemption>, RepositoryError> {
    let mut delays = RETRY_DELAYS.iter();
    loop {
        match record(repo, event.clone(), Timestamp::now()).await {
            Err(e) => {
                let Some(delay) = delays.next() else {
                    return Err(e);
                };
                tracing::warn!(code = "redemption.storage_failed", error = %e, retry_in = ?delay, "retrying");
                tokio::time::sleep(*delay).await;
            }
            done => return done,
        }
    }
}

/// Stores a new redemption, or applies the status of one already stored. Twitch may send the same event twice, or
/// an update before the redemption itself; the update carries the whole redemption, so it can create the record.
/// Returns the record when it was created or its status changed.
pub(crate) async fn record<R: RedemptionRepository>(
    repo: &R,
    event: ChannelPointsRedemption,
    now: Timestamp,
) -> Result<Option<Redemption>, RepositoryError> {
    let twitch_id = TwitchRedemptionId(event.id);
    let status = status(event.status);
    let redeemed_at = event.redeemed_at.parse::<Timestamp>()
        .unwrap_or_else(|_| {
            tracing::warn!(redemption = %twitch_id.0, "unreadable redemption time; using the time received");
            now
        });
    let new = Redemption::record(
        twitch_id.clone(),
        Viewer {
            id: event.user_id,
            login: event.user_login,
            name: event.user_name,
        },
        Reward {
            id: event.reward_id,
            title: event.reward_title,
            cost: event.reward_cost,
        },
        event.user_input,
        status,
        redeemed_at,
        now,
    );
    if repo.insert(&new).await? {
        return Ok(Some(new));
    }
    // Already stored: a repeated `add` changes nothing; an update goes through the status rules.
    if event.change == RedemptionChange::Added {
        return Ok(None);
    }
    let mut existing = repo
        .find_by_twitch_id(&twitch_id)
        .await?
        .ok_or_else(|| RepositoryError("record vanished after a duplicate insert".into()))?;
    match existing.change_status(status, now) {
        Ok(true) => repo.save_status(&existing).await.map(|()| Some(existing)),
        Ok(false) => Ok(None),
        Err(e) => {
            tracing::warn!(
                code = %e,
                redemption = %twitch_id.0,
                from = ?e.from,
                to = ?e.to,
                "redemption status change rejected"
            );
            Ok(None)
        }
    }
}

/// Filters and page of the record list; `to` is exclusive. Text filters match part of the name, ignoring case.
#[derive(Debug, Default)]
pub(crate) struct ListFilter {
    pub(crate) from: Option<Timestamp>,
    pub(crate) to: Option<Timestamp>,
    /// Login or display name.
    pub(crate) viewer: Option<String>,
    pub(crate) reward: Option<String>,
    pub(crate) status: Option<Status>,
    pub(crate) offset: u32,
    pub(crate) limit: u32,
}

fn status(s: RedemptionStatus) -> Status {
    match s {
        RedemptionStatus::Unfulfilled => Status::Unfulfilled,
        RedemptionStatus::Fulfilled => Status::Fulfilled,
        RedemptionStatus::Canceled => Status::Canceled,
        RedemptionStatus::Unknown => Status::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use toolkit_core::events::{RedemptionChange::Added, RedemptionStatus::Unfulfilled};

    use super::*;
    use crate::infrastructure::db::tests::{event, modules};

    /// Fails the first `failures` inserts, then stores.
    struct Flaky {
        failures: AtomicU64,
        inserts: Arc<Mutex<u64>>,
    }

    impl RedemptionRepository for Flaky {
        async fn insert(&self, _: &Redemption) -> Result<bool, RepositoryError> {
            *self.inserts.lock().unwrap() += 1;
            if self.failures.load(Ordering::SeqCst) == 0 {
                return Ok(true);
            }
            self.failures.fetch_sub(1, Ordering::SeqCst);
            Err(RepositoryError("database is locked".into()))
        }
        async fn find_by_twitch_id(
            &self,
            _: &TwitchRedemptionId,
        ) -> Result<Option<Redemption>, RepositoryError> {
            unreachable!()
        }
        async fn save_status(&self, _: &Redemption) -> Result<(), RepositoryError> {
            unreachable!()
        }
    }

    async fn run(failures: u64, name: &str) -> (RedemptionService, u64, Vec<String>) {
        let inserts = Arc::new(Mutex::new(0));
        let stored = Arc::new(Mutex::new(Vec::new()));
        let seen = stored.clone();
        let repo = Flaky {
            failures: AtomicU64::new(failures),
            inserts: inserts.clone(),
        };
        let service = RedemptionService::start(repo, modules(name), move |r| {
            seen.lock().unwrap().push(r.twitch_id.0.clone())
        });
        service.receive(event("r-1", Added, Unfulfilled), Source::Twitch);
        // Paused time: sleeping past every retry delay runs them all at once.
        tokio::time::sleep(Duration::from_secs(60)).await;
        let inserts = *inserts.lock().unwrap();
        let stored = stored.lock().unwrap().clone();
        (service, inserts, stored)
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_write_is_retried_until_it_goes_through() {
        let (service, inserts, stored) = run(3, "retry").await;
        assert_eq!(inserts, 4);
        assert_eq!(stored, ["r-1"]);
        assert_eq!(service.failed_writes(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_write_still_failing_after_the_retries_is_counted() {
        let (service, inserts, stored) = run(u64::MAX, "gave-up").await;
        assert_eq!(inserts, 1 + RETRY_DELAYS.len() as u64);
        assert!(stored.is_empty());
        assert_eq!(service.failed_writes(), 1);
    }
}
