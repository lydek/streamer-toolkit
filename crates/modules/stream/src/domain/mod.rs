//! What a start, an end or a check does to the current session.

use toolkit_core::events::StreamChange;
use toolkit_core::time::{SignedDuration, Timestamp};

/// Get Streams can still say "not live" this long after a stream started; a check that soon is not an end.
pub(crate) const START_LAG: SignedDuration = SignedDuration::from_mins(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Session {
    pub(crate) id: String,
    pub(crate) twitch_id: String,
    pub(crate) started_at: Timestamp,
    pub(crate) ended_at: Option<Timestamp>,
}

impl Session {
    pub(crate) fn live(&self) -> bool {
        self.ended_at.is_none()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// A stream this session is not: open its session, or reopen the one with that Twitch ID.
    Open {
        twitch_id: String,
        started_at: Timestamp,
    },
    /// The live stream ended now.
    End,
    /// The session stays as it is.
    Keep,
}

pub(crate) fn step(current: Option<&Session>, change: &StreamChange, now: Timestamp) -> Step {
    let live = current.filter(|s| s.live());
    let open = |id: &str, started_at: &str| {
        // Twitch always sends RFC 3339; if not, the stream started about now.
        let started_at = started_at.parse::<Timestamp>().map_or(now, |t| t);
        match current {
            // A late check (or start) for a stream older than the current session never brings it back.
            Some(c) if started_at < c.started_at => Step::Keep,
            _ => Step::Open {
                twitch_id: id.to_owned(),
                started_at,
            },
        }
    };
    match change {
        StreamChange::Online { id, started_at } => match live {
            Some(s) if s.twitch_id == *id => Step::Keep,
            _ => open(id, started_at),
        },
        StreamChange::Offline => match live {
            Some(_) => Step::End,
            None => Step::Keep,
        },
        // A check still listing a stream that already ended (or the live one) changes nothing.
        StreamChange::Checked(Some(s)) => match current {
            Some(c) if c.twitch_id == s.id => Step::Keep,
            _ => open(&s.id, &s.started_at),
        },
        StreamChange::Checked(None) => match live {
            Some(s) if now - s.started_at >= START_LAG => Step::End,
            _ => Step::Keep,
        },
        StreamChange::SignedOut => Step::Keep,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use toolkit_core::events::LiveStream;

    fn at(s: &str) -> Timestamp {
        s.parse::<Timestamp>().unwrap()
    }

    fn session(twitch_id: &str, ended: bool) -> Session {
        Session {
            id: "s".into(),
            twitch_id: twitch_id.into(),
            started_at: at("2026-11-19T10:00:00Z"),
            ended_at: ended.then(|| at("2026-11-19T12:00:00Z")),
        }
    }

    fn checked(id: &str) -> StreamChange {
        StreamChange::Checked(Some(LiveStream {
            id: id.into(),
            started_at: "2026-11-19T10:00:00Z".into(),
            viewers: 5,
        }))
    }

    const NOW: &str = "2026-11-19T10:10:00Z";

    #[test]
    fn a_start_opens_a_session_once() {
        let online = StreamChange::Online {
            id: "1".into(),
            started_at: "2026-11-19T10:00:00.5Z".into(),
        };
        let opened = Step::Open {
            twitch_id: "1".into(),
            started_at: at("2026-11-19T10:00:00.5Z"),
        };
        assert_eq!(step(None, &online, at(NOW)), opened);
        assert_eq!(step(Some(&session("0", true)), &online, at(NOW)), opened);
        assert_eq!(
            step(Some(&session("1", false)), &online, at(NOW)),
            Step::Keep,
            "the same stream twice is one session"
        );
    }

    #[test]
    fn an_end_ends_only_a_live_session_and_it_stays_current() {
        assert_eq!(
            step(Some(&session("1", false)), &StreamChange::Offline, at(NOW)),
            Step::End
        );
        assert_eq!(
            step(Some(&session("1", true)), &StreamChange::Offline, at(NOW)),
            Step::Keep
        );
        assert_eq!(step(None, &StreamChange::Offline, at(NOW)), Step::Keep);
    }

    #[test]
    fn a_check_catches_a_stream_already_live_and_lags_are_ignored() {
        // Started mid-stream: Twitch's own start time.
        assert_eq!(
            step(None, &checked("1"), at(NOW)),
            Step::Open {
                twitch_id: "1".into(),
                started_at: at("2026-11-19T10:00:00Z")
            }
        );
        assert_eq!(
            step(Some(&session("1", false)), &checked("1"), at(NOW)),
            Step::Keep
        );
        assert_eq!(
            step(Some(&session("1", true)), &checked("1"), at(NOW)),
            Step::Keep,
            "a check still listing the stream that just ended does not reopen it"
        );
        assert_eq!(
            step(Some(&session("1", true)), &checked("2"), at(NOW)),
            Step::Open {
                twitch_id: "2".into(),
                started_at: at("2026-11-19T10:00:00Z")
            }
        );
    }

    #[test]
    fn a_check_ends_a_missed_end_but_not_a_fresh_start() {
        let none = StreamChange::Checked(None);
        let live = session("1", false);
        assert_eq!(
            step(Some(&live), &none, at("2026-11-19T10:02:59Z")),
            Step::Keep
        );
        assert_eq!(
            step(Some(&live), &none, at("2026-11-19T10:03:00Z")),
            Step::End
        );
        assert_eq!(step(Some(&session("1", true)), &none, at(NOW)), Step::Keep);
    }

    #[test]
    fn a_late_check_for_an_older_stream_changes_nothing() {
        // B started an hour after A; a check that still lists A arrives after B's start.
        let b = Session {
            id: "b".into(),
            twitch_id: "B".into(),
            started_at: at("2026-11-19T11:00:00Z"),
            ended_at: None,
        };
        assert_eq!(
            step(Some(&b), &checked("A"), at("2026-11-19T11:01:00Z")),
            Step::Keep
        );
        let old_start = StreamChange::Online {
            id: "A".into(),
            started_at: "2026-11-19T10:00:00Z".into(),
        };
        assert_eq!(
            step(Some(&b), &old_start, at("2026-11-19T11:01:00Z")),
            Step::Keep
        );
    }

    #[test]
    fn signing_out_proves_nothing_about_the_stream() {
        assert_eq!(
            step(
                Some(&session("1", false)),
                &StreamChange::SignedOut,
                at(NOW)
            ),
            Step::Keep
        );
    }
}
