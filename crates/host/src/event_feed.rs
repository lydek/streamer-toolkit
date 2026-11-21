//! The home page's event feed: alerts, redemptions and check-ins of this stream (or since launch before the first
//! stream), newest first, each with what it set off and what became of that while this launch remembers it. The
//! activity, redemption and checkin modules each answer for their own table; the outcomes come from the alerts,
//! sound and checkin modules' in-memory lists. Nothing joins across modules. Admin route; the caller adds the token
//! check.

use std::{collections::HashMap, sync::Arc};

use alerts::AlertsService;
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use checkin::{CheckedInEntry, CheckinService};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sound::SoundService;
use sqlx::SqlitePool;
use toolkit_core::{
    events::{Alert, AlertKind, Source},
    presentation::{Outcome, RecentItem, Subject},
};
use utoipa::ToSchema;

use activity::ActivityEntry;
use redemption::RedeemedEntry;

use crate::admin::{ErrorBody, error};

/// How many items the feed lists.
const ITEMS: usize = 50;

#[derive(Clone)]
pub(crate) struct FeedState {
    pub(crate) pool: SqlitePool,
    /// Before the first stream, the feed starts here.
    pub(crate) launched_at: DateTime<Utc>,
    pub(crate) alerts: Arc<AlertsService>,
    pub(crate) sounds: Arc<SoundService>,
    pub(crate) checkin: Arc<CheckinService>,
}

pub(crate) fn routes(state: FeedState) -> Router {
    Router::new()
        .route("/api/admin/feed", get(event_feed))
        .with_state(state)
}

/// What happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum FeedEvent {
    /// `alert` is its kind as in the `alert` payload (`follow`, `subscribe`, ...); `amount` the months of a resub,
    /// the subs of a gift, the bits of a cheer or the viewers of a raid.
    Alert { alert: String, amount: Option<u32> },
    /// The reward's title (raw text).
    Redemption { reward: String },
    /// `chat`, `redemption` or `manual`; `simulation` for a card from the simulator.
    Checkin { trigger: String },
}

/// What it set off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Triggered {
    Alert,
    Sound,
    Card,
}

/// What became of what it set off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FeedOutcome {
    Queued,
    Played,
    SoundMissing,
    OffAir,
    Declined,
    Skipped,
    Cleared,
    Stopped,
}

impl From<Option<Outcome>> for FeedOutcome {
    fn from(o: Option<Outcome>) -> Self {
        match o {
            None => Self::Queued,
            Some(Outcome::Played) => Self::Played,
            Some(Outcome::SoundMissing) => Self::SoundMissing,
            Some(Outcome::OffAir) => Self::OffAir,
            Some(Outcome::Declined) => Self::Declined,
            Some(Outcome::Skipped) => Self::Skipped,
            Some(Outcome::Cleared) => Self::Cleared,
            Some(Outcome::Stopped) => Self::Stopped,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub(crate) struct FeedResult {
    pub(crate) triggered: Triggered,
    /// The reward sound's name (raw text), for a sound.
    #[schema(required)]
    pub(crate) sound: Option<String>,
    pub(crate) outcome: FeedOutcome,
}

/// One item. `viewer` is raw viewer text: render it as text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub(crate) struct FeedItem {
    /// RFC 3339, UTC.
    pub(crate) at: String,
    /// `null` for an anonymous gift or cheer.
    #[schema(required)]
    pub(crate) viewer: Option<String>,
    pub(crate) event: FeedEvent,
    /// What it set off and what became of it; `null` when this launch does not know (it happened before a restart,
    /// or set off nothing).
    #[schema(required)]
    pub(crate) result: Option<FeedResult>,
    /// From the simulator (shown as 測試); never in the records.
    pub(crate) simulated: bool,
}

/// This stream's (or since launch, before the first stream) latest 50 alerts, redemptions and check-ins, newest
/// first.
#[utoipa::path(
    get,
    path = "/api/admin/feed",
    tag = "admin",
    responses(
        (status = 200, body = Vec<FeedItem>),
        (status = 500, description = "`feed.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn event_feed(State(s): State<FeedState>) -> Response {
    match read(&s).await {
        Ok(feed) => Json(feed).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "event feed not read");
            error(StatusCode::INTERNAL_SERVER_ERROR, "feed.storage_failed")
        }
    }
}

async fn read(s: &FeedState) -> Result<Vec<FeedItem>, sqlx::Error> {
    let from = stream::latest_session(&s.pool)
        .await?
        .map_or(s.launched_at, |session| session.started_at);
    let limit = ITEMS as u32;
    Ok(compose(
        from,
        Records {
            activity: activity::activity_between(&s.pool, Some(from), None, limit).await?,
            redemptions: redemption::redeemed_between(&s.pool, Some(from), None, limit).await?,
            checkins: checkin::checked_in_between(&s.pool, Some(from), None, limit).await?,
        },
        Remembered {
            alerts: s
                .alerts
                .recent()
                .into_iter()
                .map(|r| RememberedAlert {
                    alert: r.alert,
                    source: r.source,
                    outcome: r.outcome,
                })
                .collect(),
            sounds: s.sounds.recent(),
            cards: s.checkin.recent(),
        },
    ))
}

/// What the modules stored.
pub(crate) struct Records {
    pub(crate) activity: Vec<ActivityEntry>,
    pub(crate) redemptions: Vec<RedeemedEntry>,
    pub(crate) checkins: Vec<CheckedInEntry>,
}

/// An alert of this launch and what became of it.
pub(crate) struct RememberedAlert {
    pub(crate) alert: Alert,
    pub(crate) source: Source,
    pub(crate) outcome: Option<Outcome>,
}

/// What this launch remembers of what was set off.
pub(crate) struct Remembered {
    pub(crate) alerts: Vec<RememberedAlert>,
    pub(crate) sounds: Vec<RecentItem>,
    pub(crate) cards: Vec<RecentItem>,
}

fn parse(at: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(at).ok().map(|t| t.to_utc())
}

fn time_text(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn amount(kind: &str, e: &ActivityEntry) -> Option<u32> {
    match kind {
        "resubscribe" => e.months,
        "gift" => e.count,
        "cheer" => e.bits,
        "raid" => e.viewers,
        _ => None,
    }
}

fn alert_amount(kind: &AlertKind) -> Option<u32> {
    match kind {
        AlertKind::Resubscribe { months, .. } => Some(*months),
        AlertKind::Gift { count, .. } => Some(*count),
        AlertKind::Cheer { bits, .. } => Some(*bits),
        AlertKind::Raid { viewers } => Some(*viewers),
        AlertKind::Follow | AlertKind::Subscribe { .. } => None,
    }
}

/// Merges the records with what this launch remembers, adds the simulations (kept only in memory), newest first.
pub(crate) fn compose(
    from: DateTime<Utc>,
    records: Records,
    remembered: Remembered,
) -> Vec<FeedItem> {
    let alert_outcomes: HashMap<&str, Option<Outcome>> = remembered
        .alerts
        .iter()
        .map(|r| (r.alert.id.as_str(), r.outcome))
        .collect();
    let by_key = |items: &[RecentItem]| -> HashMap<String, RecentItem> {
        items
            .iter()
            .filter_map(|r| r.key.clone().map(|k| (k, r.clone())))
            .collect()
    };
    let (sounds, cards) = (by_key(&remembered.sounds), by_key(&remembered.cards));
    let mut items: Vec<(DateTime<Utc>, FeedItem)> = Vec::new();
    let mut push = |at: &str, item: FeedItem| {
        if let Some(t) = parse(at).filter(|t| *t >= from) {
            items.push((
                t,
                FeedItem {
                    at: time_text(t),
                    ..item
                },
            ));
        }
    };

    // A gift's subs are stored one by one too; the gift itself stands for them.
    for e in records.activity.iter().filter(|e| !e.gifted) {
        push(
            &e.occurred_at,
            FeedItem {
                at: String::new(),
                viewer: e.user_name.clone(),
                event: FeedEvent::Alert {
                    alert: e.kind.clone(),
                    amount: amount(&e.kind, e),
                },
                result: alert_outcomes.get(e.alert_id.as_str()).map(|o| FeedResult {
                    triggered: Triggered::Alert,
                    sound: None,
                    outcome: (*o).into(),
                }),
                simulated: false,
            },
        );
    }
    for r in &records.redemptions {
        push(
            &r.redeemed_at,
            FeedItem {
                at: String::new(),
                viewer: Some(r.user_name.clone()),
                event: FeedEvent::Redemption {
                    reward: r.reward_title.clone(),
                },
                result: sounds.get(&r.twitch_redemption_id).map(|s| FeedResult {
                    triggered: Triggered::Sound,
                    sound: match &s.subject {
                        Subject::Sound { sound, .. } => Some(sound.clone()),
                        _ => None,
                    },
                    outcome: s.outcome.into(),
                }),
                simulated: false,
            },
        );
    }
    for c in &records.checkins {
        push(
            &c.occurred_at,
            FeedItem {
                at: String::new(),
                viewer: Some(c.user_name.clone()),
                event: FeedEvent::Checkin {
                    trigger: c.trigger.clone(),
                },
                result: cards.get(&c.record_id).map(|card| FeedResult {
                    triggered: Triggered::Card,
                    sound: None,
                    outcome: card.outcome.into(),
                }),
                simulated: false,
            },
        );
    }

    // Simulations are never stored; only this launch's memory has them.
    for r in remembered
        .alerts
        .iter()
        .filter(|r| r.source == Source::Simulation)
    {
        push(
            &r.alert.occurred_at,
            FeedItem {
                at: String::new(),
                viewer: r.alert.user_name.clone(),
                event: FeedEvent::Alert {
                    alert: r.alert.kind.name().into(),
                    amount: alert_amount(&r.alert.kind),
                },
                result: Some(FeedResult {
                    triggered: Triggered::Alert,
                    sound: None,
                    outcome: r.outcome.into(),
                }),
                simulated: true,
            },
        );
    }
    for r in remembered
        .cards
        .iter()
        .filter(|r| r.source == Source::Simulation)
    {
        let Subject::Card { viewer, .. } = &r.subject else {
            continue;
        };
        push(
            &r.queued_at,
            FeedItem {
                at: String::new(),
                viewer: Some(viewer.clone()),
                event: FeedEvent::Checkin {
                    trigger: "simulation".into(),
                },
                result: Some(FeedResult {
                    triggered: Triggered::Card,
                    sound: None,
                    outcome: r.outcome.into(),
                }),
                simulated: true,
            },
        );
    }

    items.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    items.truncate(ITEMS);
    items.into_iter().map(|(_, item)| item).collect()
}

#[cfg(test)]
mod tests;
