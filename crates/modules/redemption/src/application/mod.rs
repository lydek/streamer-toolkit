//! Records redemptions from Twitch: one record per Twitch redemption ID, status changes checked by the aggregate.
//! Events are handled one at a time, in arrival order.

use std::{future::Future, sync::Arc};

use chrono::{DateTime, Utc};
use tokio::sync::mpsc;
use toolkit_core::{
    events::{ChannelPointsRedemption, RedemptionChange, RedemptionStatus, Source},
    modules::{self, Modules},
};

use crate::domain::{Redemption, Reward, Status, TwitchRedemptionId, Viewer};

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

pub struct RedemptionService {
    tx: mpsc::UnboundedSender<ChannelPointsRedemption>,
    modules: Arc<Modules>,
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
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                match record(&repo, event, Utc::now()).await {
                    Ok(Some(r)) => changed(&r),
                    Ok(None) => {}
                    Err(e) => {
                        tracing::error!(code = "redemption.storage_failed", error = %e, "redemption not stored")
                    }
                }
            }
        });
        Self { tx, modules }
    }

    /// Simulated redemptions never go into the records. While the module is disabled, redemptions are dropped
    /// like other in-memory events (the outbox that could keep them comes with the check-in slice).
    pub fn receive(&self, redemption: ChannelPointsRedemption, source: Source) {
        if source == Source::Simulation || !self.modules.is_enabled(modules::REDEMPTION) {
            return;
        }
        let _ = self.tx.send(redemption);
    }
}

/// Stores a new redemption, or applies the status of one already stored. Twitch may send the same event twice, or
/// an update before the redemption itself; the update carries the whole redemption, so it can create the record.
/// Returns the record when it was created or its status changed.
pub(crate) async fn record<R: RedemptionRepository>(
    repo: &R,
    event: ChannelPointsRedemption,
    now: DateTime<Utc>,
) -> Result<Option<Redemption>, RepositoryError> {
    let twitch_id = TwitchRedemptionId(event.id);
    let status = status(event.status);
    let redeemed_at = DateTime::parse_from_rfc3339(&event.redeemed_at)
        .map(|t| t.to_utc())
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
    pub(crate) from: Option<DateTime<Utc>>,
    pub(crate) to: Option<DateTime<Utc>>,
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
