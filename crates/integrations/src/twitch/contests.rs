//! Polls and predictions as EventSub notifications and Helix answers carry them. Both sources have the same fields
//! except the status (EventSub: lowercase, and none until the end, or the lock; Helix: uppercase) and the times
//! (EventSub: `ends_at`/`locks_at`, then `ended_at`/`locked_at`; Helix: the start plus the duration or window).

use serde::Deserialize;
use toolkit_core::events::{
    Poll, PollChoice, PollStatus, Prediction, PredictionOutcome, PredictionStatus,
};
use toolkit_core::time::{self, SignedDuration, Timestamp};

#[derive(Deserialize)]
struct PollJson {
    id: String,
    title: String,
    choices: Vec<ChoiceJson>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    ends_at: Option<String>,
    #[serde(default)]
    ended_at: Option<String>,
    #[serde(default)]
    started_at: Option<String>,
    /// Seconds (Helix).
    #[serde(default)]
    duration: Option<i64>,
}

#[derive(Deserialize)]
struct ChoiceJson {
    id: String,
    title: String,
    /// Absent at the start (EventSub `channel.poll.begin`).
    #[serde(default)]
    votes: u64,
}

/// A poll from EventSub (`channel.poll.*`) or Helix (Get Polls, Create Poll, End Poll); `None` when it does not
/// parse.
pub fn poll_from_json(value: serde_json::Value) -> Option<Poll> {
    let p: PollJson = serde_json::from_value(value).ok()?;
    let status = match p.status.as_deref().map(str::to_ascii_lowercase).as_deref() {
        // EventSub sends a status only when the poll ends.
        None | Some("active") => PollStatus::Active,
        Some("completed") => PollStatus::Completed,
        Some("terminated") => PollStatus::Terminated,
        // Archived, or moderated or invalid (Twitch removed it): not shown.
        Some(_) => PollStatus::Archived,
    };
    let planned_end = || plus_seconds(p.started_at.as_deref(), p.duration);
    let ends_at = p
        .ended_at
        .clone()
        .filter(|t| !t.is_empty())
        .or_else(|| p.ends_at.clone().filter(|t| !t.is_empty()))
        .or_else(planned_end)
        .unwrap_or_default();
    Some(Poll {
        id: p.id,
        title: p.title,
        choices: p
            .choices
            .into_iter()
            .map(|c| PollChoice {
                id: c.id,
                title: c.title,
                votes: c.votes,
            })
            .collect(),
        status,
        ends_at,
    })
}

/// `start` (RFC 3339) plus `seconds`, as RFC 3339; `None` when either is missing or does not parse.
fn plus_seconds(start: Option<&str>, seconds: Option<i64>) -> Option<String> {
    let start = start?.parse::<Timestamp>().ok()?;
    let end = start
        .checked_add(SignedDuration::from_secs(seconds?))
        .ok()?;
    Some(time::millis(end))
}

#[derive(Deserialize)]
struct PredictionJson {
    id: String,
    title: String,
    outcomes: Vec<OutcomeJson>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    winning_outcome_id: Option<String>,
    #[serde(default)]
    locks_at: Option<String>,
    #[serde(default)]
    locked_at: Option<String>,
    #[serde(default)]
    ended_at: Option<String>,
    /// Helix: when it started, and how many seconds predicting stays open.
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    prediction_window: Option<i64>,
}

#[derive(Deserialize)]
struct OutcomeJson {
    id: String,
    title: String,
    /// Absent at the start (EventSub `channel.prediction.begin`).
    #[serde(default)]
    users: u64,
    #[serde(default)]
    channel_points: u64,
}

/// A prediction from EventSub (`channel.prediction.*`, whose lock notification carries no status: `locked` says
/// it is one) or Helix (Get, Create and End Prediction); `None` when it does not parse.
pub fn prediction_from_json(value: serde_json::Value, locked: bool) -> Option<Prediction> {
    let p: PredictionJson = serde_json::from_value(value).ok()?;
    let status = match p.status.as_deref().map(str::to_ascii_lowercase).as_deref() {
        _ if locked => PredictionStatus::Locked,
        None | Some("active") => PredictionStatus::Active,
        Some("locked") => PredictionStatus::Locked,
        Some("resolved") => PredictionStatus::Resolved,
        // Canceled ("cancelled" too), or anything Twitch adds later: over, with the points given back.
        Some(_) => PredictionStatus::Canceled,
    };
    let locks_at = p
        .locked_at
        .clone()
        .filter(|t| !t.is_empty())
        .or_else(|| p.locks_at.clone().filter(|t| !t.is_empty()))
        .or_else(|| plus_seconds(p.created_at.as_deref(), p.prediction_window))
        .unwrap_or_default();
    Some(Prediction {
        id: p.id,
        title: p.title,
        outcomes: p
            .outcomes
            .into_iter()
            .map(|o| PredictionOutcome {
                id: o.id,
                title: o.title,
                users: o.users,
                points: o.channel_points,
            })
            .collect(),
        status,
        winning_outcome_id: p.winning_outcome_id.filter(|w| !w.is_empty()),
        locks_at,
        ended_at: p.ended_at.unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn eventsub_and_helix_predictions_read_the_same() {
        let outcomes = |users: u64| {
            json!([{ "id": "o1", "title": "能", "users": users, "channel_points": users * 100, "color": "blue" },
                   { "id": "o2", "title": "不能", "users": 0, "channel_points": 0, "color": "pink" }])
        };
        // channel.prediction.begin: nothing predicted yet, no status.
        let begin = prediction_from_json(
            json!({ "id": "d1", "title": "吃雞？", "locks_at": "2026-11-27T10:02:00Z",
                    "outcomes": [{ "id": "o1", "title": "能", "color": "blue" }, { "id": "o2", "title": "不能", "color": "pink" }] }),
            false,
        )
        .unwrap();
        assert_eq!(
            (
                begin.status,
                begin.locks_at.as_str(),
                begin.outcomes[0].users
            ),
            (PredictionStatus::Active, "2026-11-27T10:02:00Z", 0)
        );
        // channel.prediction.lock: no status field.
        let lock = prediction_from_json(
            json!({ "id": "d1", "title": "吃雞？", "outcomes": outcomes(3), "locked_at": "2026-11-27T10:01:30Z" }),
            true,
        )
        .unwrap();
        assert_eq!(
            (lock.status, lock.locks_at.as_str(), lock.outcomes[0].points),
            (PredictionStatus::Locked, "2026-11-27T10:01:30Z", 300)
        );
        // channel.prediction.end, resolved.
        let end = prediction_from_json(
            json!({ "id": "d1", "title": "吃雞？", "outcomes": outcomes(3), "status": "resolved",
                    "winning_outcome_id": "o1", "ended_at": "2026-11-27T10:05:00Z" }),
            false,
        )
        .unwrap();
        assert_eq!(
            (
                end.status,
                end.winning_outcome_id.as_deref(),
                end.ended_at.as_str()
            ),
            (
                PredictionStatus::Resolved,
                Some("o1"),
                "2026-11-27T10:05:00Z"
            )
        );
        assert_eq!(lock.ended_at, "");
        // Helix while active: the lock is the start plus the window; canceled spelled either way.
        let helix = prediction_from_json(
            json!({ "id": "d1", "title": "吃雞？", "outcomes": outcomes(1), "status": "ACTIVE",
                    "winning_outcome_id": null, "created_at": "2026-11-27T10:00:00Z", "prediction_window": 120,
                    "locked_at": null, "ended_at": null }),
            false,
        )
        .unwrap();
        assert_eq!(
            (helix.status, helix.locks_at.as_str()),
            (PredictionStatus::Active, "2026-11-27T10:02:00.000Z")
        );
        for gone in ["CANCELED", "cancelled"] {
            let p = prediction_from_json(
                json!({ "id": "d", "title": "t", "outcomes": [], "status": gone }),
                false,
            )
            .unwrap();
            assert_eq!(p.status, PredictionStatus::Canceled);
        }
    }

    #[test]
    fn eventsub_and_helix_polls_read_the_same() {
        // channel.poll.begin: no votes and no status yet.
        let begin = poll_from_json(json!({
            "id": "p1", "broadcaster_user_id": "42", "title": "下一首？",
            "choices": [{ "id": "c1", "title": "晴天" }, { "id": "c2", "title": "小幸運" }],
            "bits_voting": { "is_enabled": false, "amount_per_vote": 0 },
            "started_at": "2026-11-27T10:00:00Z", "ends_at": "2026-11-27T10:02:00Z"
        }))
        .unwrap();
        assert_eq!(begin.status, PollStatus::Active);
        assert_eq!(begin.ends_at, "2026-11-27T10:02:00Z");
        assert_eq!(begin.choices[0].votes, 0);
        // channel.poll.end, ended early.
        let end = poll_from_json(json!({
            "id": "p1", "title": "下一首？", "status": "terminated",
            "choices": [{ "id": "c1", "title": "晴天", "votes": 12, "channel_points_votes": 2 }],
            "started_at": "2026-11-27T10:00:00Z", "ended_at": "2026-11-27T10:01:00Z"
        }))
        .unwrap();
        assert_eq!(
            (end.status, end.ends_at.as_str(), end.choices[0].votes),
            (PollStatus::Terminated, "2026-11-27T10:01:00Z", 12)
        );
        // Helix, while active: the end is the start plus the duration.
        let helix = poll_from_json(json!({
            "id": "p1", "title": "下一首？", "status": "ACTIVE", "duration": 120,
            "choices": [{ "id": "c1", "title": "晴天", "votes": 3 }],
            "started_at": "2026-11-27T10:00:00Z", "ended_at": null
        }))
        .unwrap();
        assert_eq!(
            (helix.status, helix.ends_at.as_str()),
            (PollStatus::Active, "2026-11-27T10:02:00.000Z")
        );
        for gone in ["ARCHIVED", "MODERATED", "INVALID"] {
            let p =
                poll_from_json(json!({ "id": "p", "title": "t", "choices": [], "status": gone }))
                    .unwrap();
            assert_eq!(p.status, PollStatus::Archived);
        }
        assert!(poll_from_json(json!({ "title": "no id" })).is_none());
    }
}
