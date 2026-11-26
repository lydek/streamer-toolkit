//! 投票與預測: the channel's latest poll as Twitch reports it (EventSub, or a Helix answer), announced as
//! `twitch.contest` for the overlay and the admin page; the snapshot keeps the latest one. Kept in memory only:
//! Twitch keeps the polls themselves.

use std::sync::{Arc, Mutex};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use toolkit_core::{
    bus::{Event, EventBus},
    events::{Contest, Poll, PollStatus, Source},
};
use utoipa::ToSchema;

pub(crate) const CONTEST: &str = "twitch.contest";
const CONTEST_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PollStatusName {
    Active,
    /// Ended on time.
    Completed,
    /// Ended early.
    Terminated,
}

/// A choice and its votes so far (channel points votes included). `title` is raw Twitch text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PollChoiceView {
    id: String,
    title: String,
    votes: u64,
}

/// `title` is raw Twitch text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PollView {
    id: String,
    title: String,
    choices: Vec<PollChoiceView>,
    status: PollStatusName,
    /// RFC 3339: when voting ends, or ended.
    ends_at: String,
}

/// What is on: the latest poll, `None` before any or once it was archived.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContestPayload {
    #[schema(required)]
    poll: Option<PollView>,
}

/// A `twitch.contest` event exactly as `/ws/overlay` sends it whenever the poll changes.
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContestEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: ContestType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: ContestPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum ContestType {
    #[serde(rename = "twitch.contest")]
    TwitchContest,
}

fn view(p: Poll) -> Option<PollView> {
    let status = match p.status {
        PollStatus::Active => PollStatusName::Active,
        PollStatus::Completed => PollStatusName::Completed,
        PollStatus::Terminated => PollStatusName::Terminated,
        PollStatus::Archived => return None,
    };
    Some(PollView {
        id: p.id,
        title: p.title,
        choices: p
            .choices
            .into_iter()
            .map(|c| PollChoiceView {
                id: c.id,
                title: c.title,
                votes: c.votes,
            })
            .collect(),
        status,
        ends_at: p.ends_at,
    })
}

pub(crate) struct Contests {
    bus: Arc<EventBus>,
    current: Mutex<ContestPayload>,
}

impl Contests {
    pub(crate) fn new(bus: Arc<EventBus>) -> Self {
        bus.retain(CONTEST, 1);
        Self {
            bus,
            current: Mutex::default(),
        }
    }

    pub(crate) fn current(&self) -> ContestPayload {
        self.current.lock().unwrap().clone()
    }

    /// Takes the latest state of a poll (each report carries all of it) and announces it when it changed.
    pub(crate) fn receive(&self, contest: Contest) {
        let mut current = self.current.lock().unwrap();
        let next = match contest {
            Contest::Poll(p) => ContestPayload { poll: view(p) },
        };
        if *current == next {
            return;
        }
        *current = next.clone();
        self.bus.publish(Event {
            id: uuid::Uuid::now_v7().to_string(),
            event_type: CONTEST,
            version: CONTEST_VERSION,
            occurred_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            source: Source::Twitch,
            payload: serde_json::to_value(&next).expect("contest serializes"),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use toolkit_core::events::PollChoice;

    fn poll(status: PollStatus, votes: u64) -> Contest {
        Contest::Poll(Poll {
            id: "p1".into(),
            title: "下一首？".into(),
            choices: vec![PollChoice {
                id: "c1".into(),
                title: "晴天".into(),
                votes,
            }],
            status,
            ends_at: "2026-11-27T10:02:00Z".into(),
        })
    }

    #[test]
    fn a_poll_is_announced_when_it_changes_and_gone_once_archived() {
        let bus = Arc::new(EventBus::default());
        let contests = Contests::new(bus.clone());
        let (_, mut published) = bus.subscribe();
        contests.receive(poll(PollStatus::Active, 0));
        // The same state again (EventSub repeats, a Helix read) is not announced twice.
        contests.receive(poll(PollStatus::Active, 0));
        contests.receive(poll(PollStatus::Active, 3));
        let mut count = 0;
        while published.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 2);
        let wire = serde_json::to_value(contests.current()).unwrap();
        assert_eq!(wire["poll"]["choices"][0]["votes"], 3);
        assert_eq!(wire["poll"]["endsAt"], "2026-11-27T10:02:00Z");
        contests.receive(poll(PollStatus::Archived, 3));
        assert_eq!(contests.current(), ContestPayload::default());
    }
}
