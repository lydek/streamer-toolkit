//! Check-in rules (docs/modules/checkin.md): one check-in per viewer per business day, the business day judged in
//! the broadcaster's time zone from the time the check-in happened, and a stamp card derived from the total.

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use uuid::Uuid;

/// Twitch's numeric user ID: the key that identifies a viewer across name changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TwitchUserId(pub(crate) String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Viewer {
    pub(crate) twitch_id: TwitchUserId,
    pub(crate) login: String,
    pub(crate) name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Trigger {
    Chat,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("checkin.invalid_settings")]
pub(crate) struct InvalidSettings;

/// When a business day starts and which day an instant belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BusinessDay {
    tz: Tz,
    reset_hour: u32,
}

impl BusinessDay {
    pub(crate) fn new(time_zone: &str, reset_hour: u32) -> Result<Self, InvalidSettings> {
        let tz = time_zone.parse::<Tz>().map_err(|_| InvalidSettings)?;
        if reset_hour > 23 {
            return Err(InvalidSettings);
        }
        Ok(Self { tz, reset_hour })
    }

    pub(crate) fn time_zone(&self) -> &'static str {
        self.tz.name()
    }

    pub(crate) fn reset_hour(&self) -> u32 {
        self.reset_hour
    }

    /// The local calendar date, counted from the reset hour instead of midnight.
    pub(crate) fn day_of(&self, at: DateTime<Utc>) -> NaiveDate {
        let local = at.with_timezone(&self.tz).naive_local();
        (local - Duration::hours(i64::from(self.reset_hour))).date()
    }

    /// `[start, end)` of `day` in UTC.
    pub(crate) fn window(&self, day: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
        let start =
            |d: NaiveDate| self.instant(d.and_hms_opt(self.reset_hour, 0, 0).expect("hour <= 23"));
        (start(day), start(day + Duration::days(1)))
    }

    /// The first instant at or after a local wall-clock time; a time skipped by a DST jump moves to when the clock
    /// reads it again, a repeated one takes its first occurrence.
    fn instant(&self, mut local: NaiveDateTime) -> DateTime<Utc> {
        loop {
            if let Some(t) = self.tz.from_local_datetime(&local).earliest() {
                return t.to_utc();
            }
            local += Duration::minutes(15);
        }
    }
}

/// The chat command a viewer types, kept without its `!`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Command(String);

impl Command {
    pub(crate) const MAX_CHARS: usize = 25;

    /// Takes the command with or without its leading `!`; it must be one word.
    pub(crate) fn new(text: &str) -> Result<Self, InvalidSettings> {
        let word = text.trim();
        let word = word.strip_prefix('!').unwrap_or(word);
        let chars = word.chars().count();
        if chars == 0
            || chars > Self::MAX_CHARS
            || word.chars().any(|c| c.is_whitespace() || c == '!')
        {
            return Err(InvalidSettings);
        }
        Ok(Self(word.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// The message's first word is `!<command>`, ignoring case.
    pub(crate) fn matches(&self, message: &str) -> bool {
        message
            .split_whitespace()
            .next()
            .and_then(|w| w.strip_prefix('!'))
            .is_some_and(|w| w.to_lowercase() == self.0.to_lowercase())
    }
}

/// Stamp card slots; the card itself is derived from the total, never stored (docs/modules/checkin.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CardSlots(u32);

impl CardSlots {
    pub(crate) const MAX: u32 = 100;

    pub(crate) fn new(slots: u32) -> Result<Self, InvalidSettings> {
        if (1..=Self::MAX).contains(&slots) {
            Ok(Self(slots))
        } else {
            Err(InvalidSettings)
        }
    }

    pub(crate) fn get(self) -> u32 {
        self.0
    }
}

/// The check-in rules in force: command, business day and card size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rules {
    pub(crate) command: Command,
    pub(crate) day: BusinessDay,
    pub(crate) slots: CardSlots,
}

/// One check-in, for the business day `day` (judged from `occurred_at`, not from when it was processed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckIn {
    pub(crate) id: Uuid,
    pub(crate) day: NaiveDate,
    /// `[start, end)` of `day` under the rules in force; any check-in of the viewer inside it, or already labelled
    /// `day`, makes this one a repeat. Settings changed today therefore never allow a second one.
    pub(crate) window: (DateTime<Utc>, DateTime<Utc>),
    pub(crate) occurred_at: DateTime<Utc>,
    pub(crate) trigger: Trigger,
    pub(crate) recorded_at: DateTime<Utc>,
}

impl CheckIn {
    pub(crate) fn at(
        rule: &BusinessDay,
        occurred_at: DateTime<Utc>,
        trigger: Trigger,
        now: DateTime<Utc>,
    ) -> Self {
        let day = rule.day_of(occurred_at);
        Self {
            id: Uuid::now_v7(),
            day,
            window: rule.window(day),
            occurred_at,
            trigger,
            recorded_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().to_utc()
    }

    fn date(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn business_day_follows_zone_and_reset_hour() {
        let midnight = BusinessDay::new("Asia/Taipei", 0).unwrap();
        // 23:30 and 00:30 in Taipei (UTC+8) are different days with a midnight reset ...
        assert_eq!(
            midnight.day_of(utc("2026-11-16T15:30:00Z")),
            date("2026-11-16")
        );
        assert_eq!(
            midnight.day_of(utc("2026-11-16T16:30:00Z")),
            date("2026-11-17")
        );
        // ... and the same day when the day starts at 06:00.
        let six = BusinessDay::new("Asia/Taipei", 6).unwrap();
        assert_eq!(six.day_of(utc("2026-11-16T16:30:00Z")), date("2026-11-16"));
        assert_eq!(six.day_of(utc("2026-11-16T22:00:00Z")), date("2026-11-17"));
        assert_eq!(
            six.window(date("2026-11-17")),
            (utc("2026-11-16T22:00:00Z"), utc("2026-11-17T22:00:00Z"))
        );
    }

    #[test]
    fn window_survives_daylight_saving_changes() {
        // New York: 2026-03-08 02:00 does not exist, 2026-11-01 01:00 happens twice.
        let ny = BusinessDay::new("America/New_York", 2).unwrap();
        let (start, end) = ny.window(date("2026-03-08"));
        assert_eq!(
            start,
            utc("2026-03-08T07:00:00Z"),
            "skipped 02:00 starts at 03:00 EDT"
        );
        assert_eq!(end, utc("2026-03-09T06:00:00Z"));
        let one = BusinessDay::new("America/New_York", 1).unwrap();
        assert_eq!(
            one.window(date("2026-11-01")).0,
            utc("2026-11-01T05:00:00Z"),
            "first 01:00"
        );
        for at in [
            "2026-03-08T07:00:00Z",
            "2026-11-01T05:30:00Z",
            "2026-11-01T06:30:00Z",
        ] {
            let (s, e) = one.window(one.day_of(utc(at)));
            assert!(s <= utc(at) && utc(at) < e, "{at} lies in its own day");
        }
    }

    #[test]
    fn bad_settings_are_refused() {
        assert_eq!(BusinessDay::new("Mars/Olympus", 0), Err(InvalidSettings));
        assert_eq!(BusinessDay::new("UTC", 24), Err(InvalidSettings));
        assert!(CardSlots::new(0).is_err() && CardSlots::new(101).is_err());
        for bad in ["", "!", "two words", "!!x", &*"長".repeat(26)] {
            assert_eq!(Command::new(bad), Err(InvalidSettings), "{bad:?}");
        }
    }

    #[test]
    fn command_matches_the_first_word_ignoring_case() {
        let cmd = Command::new("!CheckIn").unwrap();
        assert_eq!(cmd.as_str(), "CheckIn");
        assert!(cmd.matches("!checkin"));
        assert!(cmd.matches("  !CHECKIN today please"));
        assert!(!cmd.matches("checkin"));
        assert!(!cmd.matches("hello !checkin"));
        assert!(!cmd.matches("!checkins"));
        assert!(Command::new("打卡").unwrap().matches("!打卡 嗨"));
    }

    #[test]
    fn check_in_takes_its_day_from_when_it_happened() {
        let rule = BusinessDay::new("Asia/Taipei", 0).unwrap();
        let late = CheckIn::at(
            &rule,
            utc("2026-11-16T15:59:00Z"),
            Trigger::Chat,
            utc("2026-11-17T03:00:00Z"),
        );
        assert_eq!(
            late.day,
            date("2026-11-16"),
            "processed the next day, counted for the day it happened"
        );
        assert_eq!(late.id.get_version_num(), 7);
    }
}
