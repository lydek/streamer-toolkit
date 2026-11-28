//! Check-in rules (docs/modules/checkin.md): one check-in per viewer per business day, the business day judged in
//! the broadcaster's time zone from the time the check-in happened, and a stamp card derived from the total.

use jiff::civil::{Date, DateTime};
use jiff::tz::{AmbiguousOffset, TimeZone};
use toolkit_core::time::{SignedDuration, Timestamp};

pub(crate) mod style;
use toolkit_core::commands::Allowed;
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
    Redemption,
    /// Registered or back-filled by the broadcaster.
    Manual,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("checkin.invalid_settings")]
pub(crate) struct InvalidSettings;

/// When a business day starts and which day an instant belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BusinessDay {
    tz: TimeZone,
    reset_hour: u32,
}

impl BusinessDay {
    pub(crate) fn new(time_zone: &str, reset_hour: u32) -> Result<Self, InvalidSettings> {
        let tz = TimeZone::get(time_zone).map_err(|_| InvalidSettings)?;
        // Only the zone's own spelling, as before: the name is stored and shown as given.
        if tz.iana_name() != Some(time_zone) || reset_hour > 23 {
            return Err(InvalidSettings);
        }
        Ok(Self { tz, reset_hour })
    }

    pub(crate) fn time_zone(&self) -> &str {
        self.tz.iana_name().expect("made from an IANA name")
    }

    pub(crate) fn reset_hour(&self) -> u32 {
        self.reset_hour
    }

    /// The local calendar date, counted from the reset hour instead of midnight.
    pub(crate) fn day_of(&self, at: Timestamp) -> Date {
        let local = self.tz.to_datetime(at);
        (local - SignedDuration::from_hours(i64::from(self.reset_hour))).date()
    }

    /// `[start, end)` of `day` in UTC.
    pub(crate) fn window(&self, day: Date) -> (Timestamp, Timestamp) {
        let hour = i8::try_from(self.reset_hour).expect("hour <= 23");
        let start = |d: Date| self.instant(d.at(hour, 0, 0, 0));
        (
            start(day),
            start(day.tomorrow().expect("a day before 9999-12-31")),
        )
    }

    /// The first instant at or after a local wall-clock time; a time skipped by a DST jump moves to when the clock
    /// reads it again, a repeated one takes its first occurrence.
    fn instant(&self, mut local: DateTime) -> Timestamp {
        loop {
            let offset = match self.tz.to_ambiguous_timestamp(local).offset() {
                AmbiguousOffset::Unambiguous { offset } => offset,
                AmbiguousOffset::Fold { before, .. } => before,
                AmbiguousOffset::Gap { .. } => {
                    local += SignedDuration::from_mins(15);
                    continue;
                }
            };
            return offset
                .to_timestamp(local)
                .expect("a time inside the supported range");
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

    /// Stamps on the current card. A full card shows every slot until the next check-in starts a new one.
    pub(crate) fn stamps(self, total: u64) -> u32 {
        if total == 0 {
            0
        } else {
            u32::try_from((total - 1) % u64::from(self.0)).expect("below slots") + 1
        }
    }
}

/// Stamps a viewer can spend: lifetime check-ins minus what was deducted. Never below 0, which an import that lowers
/// the total can otherwise cause; the deductions stay as they were.
pub(crate) fn available(total: u64, deducted: u64) -> u64 {
    total.saturating_sub(deducted)
}

/// Spending stamps (e.g. a song for a full card). The total and the card never change; only what is available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Deduction {
    amount: u32,
    reason: String,
}

/// An amount outside 1..=[`Deduction::MAX`], or a reason longer than [`Deduction::REASON_MAX`] characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InvalidDeduction;

impl Deduction {
    pub(crate) const MAX: u32 = 1_000_000;
    pub(crate) const REASON_MAX: usize = 100;

    /// The reason is optional; control characters are dropped and it is trimmed.
    pub(crate) fn new(amount: u32, reason: &str) -> Result<Self, InvalidDeduction> {
        let reason: String = reason.chars().filter(|c| !c.is_control()).collect();
        let reason = reason.trim().to_owned();
        if !(1..=Self::MAX).contains(&amount) || reason.chars().count() > Self::REASON_MAX {
            return Err(InvalidDeduction);
        }
        Ok(Self { amount, reason })
    }

    pub(crate) fn amount(&self) -> u32 {
        self.amount
    }

    pub(crate) fn reason(&self) -> &str {
        &self.reason
    }

    /// Whether it can be taken from `available` without going below 0.
    pub(crate) fn fits(&self, available: u64) -> bool {
        u64::from(self.amount) <= available
    }
}

/// Twitch's ID of the channel points reward that checks in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RewardId(String);

impl RewardId {
    pub(crate) fn new(id: &str) -> Result<Self, InvalidSettings> {
        let id = id.trim();
        let ok = (1..=64).contains(&id.len())
            && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        ok.then(|| Self(id.to_owned())).ok_or(InvalidSettings)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// The check-in rules in force: command and who may use it, reward, business day and card size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rules {
    pub(crate) command: Command,
    pub(crate) allowed: Allowed,
    pub(crate) reward: Option<RewardId>,
    pub(crate) day: BusinessDay,
    pub(crate) slots: CardSlots,
}

/// One check-in, for the business day `day` (judged from `occurred_at`, not from when it was processed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckIn {
    pub(crate) id: Uuid,
    pub(crate) day: Date,
    /// `[start, end)` of `day` under the rules in force; any check-in of the viewer inside it, or already labelled
    /// `day`, makes this one a repeat. Settings changed today therefore never allow a second one.
    pub(crate) window: (Timestamp, Timestamp),
    pub(crate) occurred_at: Timestamp,
    pub(crate) trigger: Trigger,
    pub(crate) recorded_at: Timestamp,
}

impl CheckIn {
    /// A back-filled check-in for `day`, placed at the start of that day.
    pub(crate) fn for_day(rule: &BusinessDay, day: Date, now: Timestamp) -> Self {
        let window = rule.window(day);
        Self {
            id: Uuid::now_v7(),
            day,
            window,
            occurred_at: window.0,
            trigger: Trigger::Manual,
            recorded_at: now,
        }
    }

    pub(crate) fn at(
        rule: &BusinessDay,
        occurred_at: Timestamp,
        trigger: Trigger,
        now: Timestamp,
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

/// Why an imported entry is left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkipReason {
    /// The key is not a numeric Twitch user ID.
    InvalidId,
    Owner,
    Excluded,
    /// `totalCheckIns` is missing, not an integer, negative, or above [`MAX_IMPORTED_TOTAL`].
    InvalidTotal,
    /// `lastCheckInDate` is not a `YYYY-MM-DD` date.
    InvalidDate,
}

/// One viewer's imported history: a baseline total (the stamp card is derived from it, never imported) and the
/// last check-in day as a plain calendar date. It never blocks checking in on that day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImportedViewer {
    pub(crate) twitch_id: TwitchUserId,
    pub(crate) name: Option<String>,
    pub(crate) profile_image: Option<String>,
    pub(crate) total: u64,
    pub(crate) last_day: Option<Date>,
}

/// An entry as read from the import file, before the rules.
pub(crate) struct ImportEntry<'a> {
    pub(crate) key: &'a str,
    /// `None` when missing or not an integer.
    pub(crate) total: Option<i64>,
    pub(crate) is_owner: bool,
    pub(crate) excluded: bool,
    /// The raw `lastCheckInDate`, when present.
    pub(crate) last_date: Option<&'a str>,
    pub(crate) name: Option<&'a str>,
    pub(crate) profile_image: Option<&'a str>,
}

/// The largest imported `totalCheckIns`: far above any real count, and small enough that the baseline plus
/// recorded check-ins always stays an SQLite integer.
pub(crate) const MAX_IMPORTED_TOTAL: u64 = 1_000_000_000;

impl ImportedViewer {
    pub(crate) fn check(e: &ImportEntry) -> Result<Self, SkipReason> {
        if e.key.is_empty() || e.key.len() > 20 || !e.key.bytes().all(|b| b.is_ascii_digit()) {
            return Err(SkipReason::InvalidId);
        }
        if e.is_owner {
            return Err(SkipReason::Owner);
        }
        if e.excluded {
            return Err(SkipReason::Excluded);
        }
        let total = e
            .total
            .and_then(|t| u64::try_from(t).ok())
            .filter(|t| *t <= MAX_IMPORTED_TOTAL)
            .ok_or(SkipReason::InvalidTotal)?;
        let last_day = e
            .last_date
            .map(|d| Date::strptime("%Y-%m-%d", d).map_err(|_| SkipReason::InvalidDate))
            .transpose()?;
        let text = |t: Option<&str>| {
            t.map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
        };
        Ok(Self {
            twitch_id: TwitchUserId(e.key.to_owned()),
            name: text(e.name),
            profile_image: text(e.profile_image),
            total,
            last_day,
        })
    }
}

#[cfg(test)]
mod deduction_tests {
    use super::*;

    #[test]
    fn available_never_goes_below_zero() {
        assert_eq!(available(10, 3), 7);
        assert_eq!(
            available(2, 5),
            0,
            "an import below what was deducted shows 0"
        );
    }

    #[test]
    fn a_deduction_takes_at_most_what_is_available() {
        let d = Deduction::new(3, "  換一首歌 ").unwrap();
        assert_eq!(d.reason(), "換一首歌");
        assert!(d.fits(3) && !d.fits(2));
        assert_eq!(Deduction::new(0, ""), Err(InvalidDeduction));
        assert_eq!(
            Deduction::new(Deduction::MAX + 1, ""),
            Err(InvalidDeduction)
        );
        assert_eq!(Deduction::new(1, &"字".repeat(101)), Err(InvalidDeduction));
        assert_eq!(Deduction::new(1, "a\u{0}b").unwrap().reason(), "ab");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn date(s: &str) -> Date {
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
        // Only the zone's own spelling, which is stored and shown as given.
        assert_eq!(BusinessDay::new("asia/taipei", 0), Err(InvalidSettings));
        assert_eq!(
            BusinessDay::new("Asia/Taipei", 0).unwrap().time_zone(),
            "Asia/Taipei"
        );
        assert!(CardSlots::new(0).is_err() && CardSlots::new(101).is_err());
        assert!(
            RewardId::new("").is_err()
                && RewardId::new("a b").is_err()
                && RewardId::new("<x>").is_err()
        );
        assert_eq!(RewardId::new(" 9a1f-22 ").unwrap().as_str(), "9a1f-22");
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
    fn stamp_card_is_derived_from_the_total() {
        let ten = CardSlots::new(10).unwrap();
        let cases = [(0, 0), (1, 1), (9, 9), (10, 10), (11, 1), (20, 10), (23, 3)];
        for (total, stamps) in cases {
            assert_eq!(ten.stamps(total), stamps, "total {total}");
        }
        assert_eq!(
            CardSlots::new(3).unwrap().stamps(23),
            2,
            "recomputed for a new size"
        );
    }

    #[test]
    fn import_entries_follow_the_rules() {
        let entry = |key, total| ImportEntry {
            key,
            total,
            is_owner: false,
            excluded: false,
            last_date: Some("2026-10-31"),
            name: Some(" Ann "),
            profile_image: None,
        };
        let ok = ImportedViewer::check(&entry("123", Some(42))).unwrap();
        assert_eq!(
            (ok.twitch_id.0.as_str(), ok.total, ok.name.as_deref()),
            ("123", 42, Some("Ann"))
        );
        assert_eq!(
            ok.last_day,
            Some(date("2026-10-31")),
            "read as a date, no zone"
        );
        assert_eq!(
            ImportedViewer::check(&entry("12a", Some(1))),
            Err(SkipReason::InvalidId)
        );
        assert_eq!(
            ImportedViewer::check(&entry("", Some(1))),
            Err(SkipReason::InvalidId)
        );
        assert_eq!(
            ImportedViewer::check(&entry("1", None)),
            Err(SkipReason::InvalidTotal)
        );
        assert_eq!(
            ImportedViewer::check(&entry("1", Some(-1))),
            Err(SkipReason::InvalidTotal)
        );
        assert!(ImportedViewer::check(&entry("1", Some(0))).is_ok());
        assert!(ImportedViewer::check(&entry("1", Some(MAX_IMPORTED_TOTAL as i64))).is_ok());
        assert_eq!(
            ImportedViewer::check(&entry("1", Some(MAX_IMPORTED_TOTAL as i64 + 1))),
            Err(SkipReason::InvalidTotal)
        );
        assert_eq!(
            ImportedViewer::check(&entry("1", Some(i64::MAX))),
            Err(SkipReason::InvalidTotal)
        );
        let owner = ImportEntry {
            is_owner: true,
            ..entry("1", Some(1))
        };
        assert_eq!(ImportedViewer::check(&owner), Err(SkipReason::Owner));
        let excluded = ImportEntry {
            excluded: true,
            ..entry("1", Some(1))
        };
        assert_eq!(ImportedViewer::check(&excluded), Err(SkipReason::Excluded));
        let bad_date = ImportEntry {
            last_date: Some("31/10/2026"),
            ..entry("1", Some(1))
        };
        assert_eq!(
            ImportedViewer::check(&bad_date),
            Err(SkipReason::InvalidDate)
        );
        let no_date = ImportEntry {
            last_date: None,
            ..entry("1", Some(1))
        };
        assert_eq!(ImportedViewer::check(&no_date).unwrap().last_day, None);
    }

    #[test]
    fn back_fill_lies_inside_its_day() {
        let rule = BusinessDay::new("Asia/Taipei", 6).unwrap();
        let c = CheckIn::for_day(&rule, date("2026-11-10"), utc("2026-11-16T00:00:00Z"));
        assert_eq!((c.day, c.trigger), (date("2026-11-10"), Trigger::Manual));
        assert_eq!(c.occurred_at, utc("2026-11-09T22:00:00Z"));
        assert_eq!(rule.day_of(c.occurred_at), c.day);
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
