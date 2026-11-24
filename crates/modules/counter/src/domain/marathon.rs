//! The sub marathon timer's rules: what adds time, the cap, and when the clock runs. The Host owns the clock (the
//! overlay never reports back): the remaining time is kept as of an anchor time and counted down from there only
//! while live and not paused.

use chrono::{DateTime, Utc};
use toolkit_core::events::AlertKind;

const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 60 * MINUTE_MS;

/// The most minutes one sub or 100 bits may add, and the longest cap, in hours (a week).
pub(crate) const MINUTES_MAX: u32 = 24 * 60;
pub(crate) const HOURS_MAX: u32 = 7 * 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarathonOptions {
    /// Added per sub, resub or gifted sub.
    pub sub_minutes: u32,
    /// Added per 100 bits, in proportion (50 bits add half).
    pub bits_minutes: u32,
    /// The remaining time never goes above this.
    pub max_hours: u32,
}

impl MarathonOptions {
    pub(crate) fn valid(self) -> bool {
        self.sub_minutes <= MINUTES_MAX
            && self.bits_minutes <= MINUTES_MAX
            && (1..=HOURS_MAX).contains(&self.max_hours)
    }
}

impl Default for MarathonOptions {
    /// Nothing adds time until the broadcaster sets it up.
    fn default() -> Self {
        Self {
            sub_minutes: 0,
            bits_minutes: 0,
            max_hours: 24,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marathon {
    /// As of `anchor`.
    pub remaining_ms: i64,
    /// When `remaining_ms` was last worked out; while live, the last time the toolkit saw the stream live.
    pub anchor: DateTime<Utc>,
    /// Paused by hand; a stream starting never clears it.
    pub paused: bool,
    /// The stream session the clock has counted in.
    pub session: Option<String>,
    pub options: MarathonOptions,
}

impl Marathon {
    pub fn new(now: DateTime<Utc>) -> Self {
        Self {
            remaining_ms: 0,
            anchor: now,
            paused: false,
            session: None,
            options: MarathonOptions::default(),
        }
    }

    fn cap_ms(&self) -> i64 {
        i64::from(self.options.max_hours) * HOUR_MS
    }

    /// Counts down from the anchor to `now` when `counting`, then moves the anchor to `now`.
    pub(crate) fn settle(&mut self, now: DateTime<Utc>, counting: bool) {
        if counting {
            let elapsed = (now - self.anchor).num_milliseconds().max(0);
            self.remaining_ms = (self.remaining_ms - elapsed).max(0);
        }
        self.anchor = now;
    }

    /// Adds (or with a negative `ms`, takes) time, staying between 0 and the cap.
    pub(crate) fn add_ms(&mut self, ms: i64) {
        self.remaining_ms = (self.remaining_ms.saturating_add(ms)).clamp(0, self.cap_ms());
    }

    /// How much an alert adds: each sub, resub and gifted sub counts once (Twitch's separate subscribe for each
    /// gifted sub is skipped, the gift's count stands for them), bits in proportion; nothing else adds time.
    pub(crate) fn for_alert(&self, kind: &AlertKind) -> i64 {
        let per_sub = i64::from(self.options.sub_minutes) * MINUTE_MS;
        match kind {
            AlertKind::Subscribe { gifted: false, .. } | AlertKind::Resubscribe { .. } => per_sub,
            AlertKind::Gift { count, .. } => per_sub * i64::from(*count),
            AlertKind::Cheer { bits, .. } => {
                i64::from(*bits) * i64::from(self.options.bits_minutes) * MINUTE_MS / 100
            }
            AlertKind::Subscribe { gifted: true, .. }
            | AlertKind::Follow
            | AlertKind::Raid { .. } => 0,
        }
    }

    /// The options, keeping the remaining time under the new cap.
    pub(crate) fn set_options(&mut self, options: MarathonOptions) {
        self.options = options;
        self.remaining_ms = self.remaining_ms.min(self.cap_ms());
    }
}

/// Whether the toolkit has seen the stream live since it started: unknown until the first status, so time while
/// it was closed is only counted once a status says the same stream is still live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Seen {
    Unknown,
    Live,
    Offline,
}

/// The clock: the marathon and what the toolkit has seen of the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Clock {
    pub(crate) marathon: Marathon,
    pub(crate) seen: Seen,
}

impl Clock {
    pub(crate) fn new(marathon: Marathon) -> Self {
        Self {
            marathon,
            seen: Seen::Unknown,
        }
    }

    /// Counting down now.
    pub(crate) fn running(&self) -> bool {
        self.seen == Seen::Live && !self.marathon.paused && self.marathon.remaining_ms > 0
    }

    /// Before the first status the anchor stays: it is the last time the stream was seen live, which the first
    /// status still needs to count the time the toolkit was closed.
    pub(crate) fn settle(&mut self, now: DateTime<Utc>) {
        if self.seen == Seen::Unknown {
            return;
        }
        let counting = self.running();
        self.marathon.settle(now, counting);
    }

    /// A live status for `session`, which started at `started_at`. The same stream counts since the anchor (also
    /// after a restart, as the stream stayed live), unless the toolkit saw it go offline since; a new stream counts
    /// from its start. A manual pause stays.
    pub(crate) fn live(&mut self, session: &str, started_at: DateTime<Utc>, now: DateTime<Utc>) {
        let m = &mut self.marathon;
        let same = m.session.as_deref() == Some(session);
        let counted = match (same, self.seen) {
            (true, Seen::Offline) => false,
            (true, _) => true,
            (false, _) => {
                m.anchor = m.anchor.max(started_at);
                true
            }
        };
        m.settle(now, counted && !m.paused);
        m.session = Some(session.to_owned());
        self.seen = Seen::Live;
    }

    /// An offline status: counts up to now if the toolkit saw the stream live, otherwise only up to the anchor (the
    /// last time it saw it live, before it was closed).
    pub(crate) fn offline(&mut self, now: DateTime<Utc>) {
        self.settle(now);
        self.seen = Seen::Offline;
    }

    /// A pause or resume before the first status gives up the time the toolkit was closed: whether the stream
    /// stayed live is not known yet, so none of it is counted.
    pub(crate) fn pause(&mut self, paused: bool, now: DateTime<Utc>) {
        if self.seen == Seen::Unknown {
            self.marathon.settle(now, false);
        }
        self.settle(now);
        self.marathon.paused = paused;
    }

    /// Before the first status, time added to a clock already at 0 starts from now: the closed time had nothing
    /// left to take, so it never takes from what is added.
    pub(crate) fn add(&mut self, ms: i64, now: DateTime<Utc>) {
        if self.seen == Seen::Unknown && self.marathon.remaining_ms == 0 {
            self.marathon.settle(now, false);
        }
        self.settle(now);
        self.marathon.add_ms(ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(min: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-21T10:00:00Z")
            .unwrap()
            .to_utc()
            + chrono::TimeDelta::minutes(min)
    }

    fn clock(remaining_min: i64) -> Clock {
        let mut m = Marathon::new(at(0));
        m.options = MarathonOptions {
            sub_minutes: 5,
            bits_minutes: 2,
            max_hours: 2,
        };
        m.remaining_ms = remaining_min * MINUTE_MS;
        Clock::new(m)
    }

    fn minutes(c: &Clock) -> i64 {
        c.marathon.remaining_ms / MINUTE_MS
    }

    #[test]
    fn subs_resubs_gifts_and_bits_add_time_up_to_the_cap() {
        let mut c = clock(0);
        let m = &c.marathon;
        assert_eq!(
            m.for_alert(&AlertKind::Subscribe {
                tier: 1,
                gifted: false
            }),
            5 * MINUTE_MS
        );
        assert_eq!(
            m.for_alert(&AlertKind::Resubscribe {
                tier: 3,
                months: 9,
                message: String::new()
            }),
            5 * MINUTE_MS
        );
        // A gift of 3 counts three times; the subscribe Twitch sends for each gifted sub counts nothing.
        assert_eq!(
            m.for_alert(&AlertKind::Gift { tier: 1, count: 3 }),
            15 * MINUTE_MS
        );
        assert_eq!(
            m.for_alert(&AlertKind::Subscribe {
                tier: 1,
                gifted: true
            }),
            0
        );
        assert_eq!(
            m.for_alert(&AlertKind::Cheer {
                bits: 150,
                message: String::new()
            }),
            3 * MINUTE_MS
        );
        assert_eq!(m.for_alert(&AlertKind::Follow), 0);
        assert_eq!(m.for_alert(&AlertKind::Raid { viewers: 50 }), 0);

        c.add(100 * MINUTE_MS, at(0));
        c.add(100 * MINUTE_MS, at(0));
        assert_eq!(minutes(&c), 120, "never above the cap");
        c.add(-500 * MINUTE_MS, at(0));
        assert_eq!(minutes(&c), 0, "never below 0");
        c.add(90 * MINUTE_MS, at(0));
        c.marathon.set_options(MarathonOptions {
            max_hours: 1,
            ..c.marathon.options
        });
        assert_eq!(minutes(&c), 60, "a lower cap trims it");
    }

    #[test]
    fn it_counts_down_only_while_live_and_not_paused() {
        let mut c = clock(60);
        c.settle(at(10));
        assert_eq!(minutes(&c), 60, "not live yet");
        c.live("s1", at(10), at(10));
        c.live("s1", at(10), at(20));
        assert_eq!(minutes(&c), 50);
        assert!(c.running());
        c.offline(at(25));
        assert_eq!(minutes(&c), 45);
        c.settle(at(40));
        assert_eq!(minutes(&c), 45, "offline");

        // A manual pause stays when the next stream starts.
        c.pause(true, at(40));
        c.live("s2", at(50), at(50));
        c.live("s2", at(50), at(70));
        assert_eq!((minutes(&c), c.running()), (45, false));
        c.pause(false, at(70));
        c.live("s2", at(50), at(75));
        assert_eq!(minutes(&c), 40);
    }

    #[test]
    fn time_while_closed_counts_only_while_the_same_stream_stayed_live() {
        // Closed during s1 at minute 10 (last seen live), restarted at 40 with s1 still live: 30 more minutes.
        let mut c = clock(60);
        c.live("s1", at(0), at(0));
        c.live("s1", at(0), at(10));
        let saved = c.marathon.clone();
        let mut restarted = Clock::new(saved.clone());
        restarted.live("s1", at(0), at(40));
        assert_eq!(minutes(&restarted), 20);

        // s1 ended while closed and s2 started at 50: s1 counts only to its last sighting, s2 from its start.
        let mut restarted = Clock::new(saved.clone());
        restarted.live("s2", at(50), at(55));
        assert_eq!(minutes(&restarted), 45);

        // s1 ended while closed and nothing is live: the offline time is not counted.
        let mut restarted = Clock::new(saved);
        restarted.offline(at(40));
        assert_eq!(minutes(&restarted), 50);
        restarted.settle(at(90));
        assert_eq!(minutes(&restarted), 50);
    }

    #[test]
    fn reads_and_changes_before_the_first_status_keep_the_closed_time() {
        // Closed during s1 at minute 10; restarted at 40, read and given 5 minutes before the first status at 45.
        let mut c = clock(60);
        c.live("s1", at(0), at(0));
        c.live("s1", at(0), at(10));
        let saved = c.marathon.clone();
        let mut restarted = Clock::new(saved.clone());
        restarted.settle(at(40));
        restarted.add(5 * MINUTE_MS, at(42));
        assert_eq!(restarted.marathon.anchor, at(10));
        restarted.live("s1", at(0), at(45));
        assert_eq!(
            minutes(&restarted),
            20,
            "50 + 5, less the 35 minutes since the last sighting"
        );

        // A new stream counts from its start, also after a read.
        let mut restarted = Clock::new(saved.clone());
        restarted.settle(at(40));
        restarted.live("s2", at(50), at(55));
        assert_eq!(minutes(&restarted), 45);

        // Offline at the first status: nothing counted.
        let mut restarted = Clock::new(saved.clone());
        restarted.settle(at(40));
        restarted.offline(at(41));
        assert_eq!(minutes(&restarted), 50);

        // At 0 when closed: time added before the first status is not taken by the closed time.
        let mut empty = Clock::new(Marathon {
            remaining_ms: 0,
            ..saved.clone()
        });
        empty.add(10 * MINUTE_MS, at(40));
        empty.live("s1", at(0), at(41));
        assert_eq!(minutes(&empty), 9);

        // Paused before closing: a restart counts nothing.
        let mut paused = c.clone();
        paused.pause(true, at(10));
        let mut restarted = Clock::new(paused.marathon);
        restarted.live("s1", at(0), at(40));
        assert_eq!((minutes(&restarted), restarted.running()), (50, false));

        // Paused and resumed before the first status: the closed time is given up, counting resumes from there.
        let mut restarted = Clock::new(saved);
        restarted.pause(true, at(40));
        restarted.pause(false, at(41));
        restarted.live("s1", at(0), at(46));
        assert_eq!(minutes(&restarted), 45);
    }

    #[test]
    fn the_same_stream_seen_offline_then_live_again_skips_the_gap() {
        let mut c = clock(60);
        c.live("s1", at(0), at(0));
        c.offline(at(10));
        c.live("s1", at(0), at(30));
        c.live("s1", at(0), at(35));
        assert_eq!(minutes(&c), 45);
    }

    #[test]
    fn it_stops_at_zero() {
        let mut c = clock(5);
        c.live("s1", at(0), at(0));
        c.live("s1", at(0), at(20));
        assert_eq!((c.marathon.remaining_ms, c.running()), (0, false));
        c.add(10 * MINUTE_MS, at(20));
        assert!(c.running(), "added time runs again while live");
    }
}
