//! Polls (and later predictions) as EventSub notifications and Helix answers carry them. Both have the same fields
//! except the status (EventSub: lowercase, and none until the end; Helix: uppercase) and the end time (EventSub:
//! `ends_at`, then `ended_at`; Helix: `started_at` plus `duration`, then `ended_at`).

use chrono::{DateTime, SecondsFormat, TimeDelta};
use serde::Deserialize;
use toolkit_core::events::{Poll, PollChoice, PollStatus};

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
    let planned_end = || {
        let start = DateTime::parse_from_rfc3339(p.started_at.as_deref()?).ok()?;
        let end = start + TimeDelta::try_seconds(p.duration?)?;
        Some(end.to_utc().to_rfc3339_opts(SecondsFormat::Millis, true))
    };
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
