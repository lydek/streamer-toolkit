//! Redemption record aggregate: one record per Twitch redemption, and the status rules it must keep
//! (docs/modules/redemption.md).

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Internal ID (UUID v7); Twitch's own ID is kept separately in [`TwitchRedemptionId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RedemptionId(pub(crate) Uuid);

impl RedemptionId {
    pub(crate) fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

/// Twitch's redemption ID: the key that keeps one record per redemption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TwitchRedemptionId(pub(crate) String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Viewer {
    pub(crate) id: String,
    pub(crate) login: String,
    pub(crate) name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reward {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) cost: u32,
}

/// `Canceled` means the points were refunded. `Unknown` is kept as-is when Twitch sends a status it did not name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    Unfulfilled,
    Fulfilled,
    Canceled,
    Unknown,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("redemption.invalid_transition")]
pub(crate) struct InvalidTransition {
    pub(crate) from: Status,
    pub(crate) to: Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Redemption {
    pub(crate) id: RedemptionId,
    pub(crate) twitch_id: TwitchRedemptionId,
    pub(crate) viewer: Viewer,
    pub(crate) reward: Reward,
    pub(crate) user_input: String,
    pub(crate) status: Status,
    pub(crate) redeemed_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
}

impl Redemption {
    /// A new record with whatever status Twitch reports first; an update can arrive before the redemption itself.
    pub(crate) fn record(
        twitch_id: TwitchRedemptionId,
        viewer: Viewer,
        reward: Reward,
        user_input: String,
        status: Status,
        redeemed_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id: RedemptionId::new(),
            twitch_id,
            viewer,
            reward,
            user_input,
            status,
            redeemed_at,
            updated_at: now,
        }
    }

    /// Only an unfulfilled redemption becomes fulfilled or canceled; a canceled one never becomes valid again.
    /// A record stored as `Unknown` takes the first known status. Returns whether the status changed.
    pub(crate) fn change_status(
        &mut self,
        to: Status,
        now: DateTime<Utc>,
    ) -> Result<bool, InvalidTransition> {
        use Status::*;
        let allowed = match (self.status, to) {
            (from, to) if from == to => return Ok(false),
            (Unfulfilled, Fulfilled | Canceled) => true,
            (Unknown, Unfulfilled | Fulfilled | Canceled) => true,
            _ => false,
        };
        if !allowed {
            return Err(InvalidTransition {
                from: self.status,
                to,
            });
        }
        self.status = to;
        self.updated_at = now;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Status::*;

    fn redemption(status: Status) -> Redemption {
        Redemption::record(
            TwitchRedemptionId("r-1".into()),
            Viewer {
                id: "7".into(),
                login: "viewer".into(),
                name: "Viewer".into(),
            },
            Reward {
                id: "w-1".into(),
                title: "Hydrate".into(),
                cost: 100,
            },
            String::new(),
            status,
            DateTime::UNIX_EPOCH,
            DateTime::UNIX_EPOCH,
        )
    }

    #[test]
    fn status_transitions() {
        let all = [Unfulfilled, Fulfilled, Canceled, Unknown];
        let allowed = [
            (Unfulfilled, Fulfilled),
            (Unfulfilled, Canceled),
            (Unknown, Unfulfilled),
            (Unknown, Fulfilled),
            (Unknown, Canceled),
        ];
        let now = DateTime::from_timestamp(60, 0).unwrap();
        for from in all {
            for to in all {
                let mut r = redemption(from);
                let result = r.change_status(to, now);
                if from == to {
                    assert_eq!(result, Ok(false), "{from:?} again is a no-op");
                    assert_eq!(r.updated_at, DateTime::UNIX_EPOCH);
                } else if allowed.contains(&(from, to)) {
                    assert_eq!(result, Ok(true), "{from:?} -> {to:?}");
                    assert_eq!((r.status, r.updated_at), (to, now));
                } else {
                    assert_eq!(
                        result,
                        Err(InvalidTransition { from, to }),
                        "{from:?} -> {to:?}"
                    );
                    assert_eq!(r.status, from, "a rejected change keeps the status");
                }
            }
        }
    }

    #[test]
    fn canceled_never_becomes_valid_again() {
        let mut r = redemption(Unfulfilled);
        r.change_status(Canceled, DateTime::UNIX_EPOCH).unwrap();
        assert!(r.change_status(Unfulfilled, DateTime::UNIX_EPOCH).is_err());
        assert!(r.change_status(Fulfilled, DateTime::UNIX_EPOCH).is_err());
        assert_eq!(r.status, Canceled);
    }

    #[test]
    fn internal_ids_are_uuid_v7() {
        assert_eq!(redemption(Unfulfilled).id.0.get_version_num(), 7);
    }
}
