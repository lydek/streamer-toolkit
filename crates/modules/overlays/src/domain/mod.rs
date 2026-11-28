//! Screen tools (thin module): plain structs with simple checks.

use toolkit_core::time::{SignedDuration, Timestamp};
pub mod appearance;
pub mod media;
pub mod warnings;

/// The longest text on the card.
pub(crate) const TEXT_MAX: usize = 100;
/// The longest countdown: a day.
pub(crate) const MINUTES_MAX: u32 = 24 * 60;

/// How the card counts down: a number of minutes, or to a time of day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountdownMode {
    Minutes,
    Until,
}

impl CountdownMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Minutes => "minutes",
            Self::Until => "until",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "minutes" => Some(Self::Minutes),
            "until" => Some(Self::Until),
            _ => None,
        }
    }
}

/// What the broadcaster set for the break card; the admin UI shows it again next time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakSettings {
    pub text: String,
    /// Shown instead of the countdown once it reaches 0.
    pub end_text: String,
    pub mode: CountdownMode,
    pub minutes: u32,
    /// `HH:MM`, local time; the admin UI turns it into the next such moment.
    pub until_time: String,
}

impl Default for BreakSettings {
    fn default() -> Self {
        Self {
            text: "休息一下，馬上回來".into(),
            end_text: "馬上回來".into(),
            mode: CountdownMode::Minutes,
            minutes: 10,
            until_time: "20:00".into(),
        }
    }
}

impl BreakSettings {
    /// Texts trimmed and at most [`TEXT_MAX`] characters (either may be empty), minutes 1 to a day, `HH:MM`.
    pub(crate) fn normalized(mut self) -> Option<Self> {
        self.text = self.text.trim().to_owned();
        self.end_text = self.end_text.trim().to_owned();
        let short = |s: &str| s.chars().count() <= TEXT_MAX;
        (short(&self.text)
            && short(&self.end_text)
            && (1..=MINUTES_MAX).contains(&self.minutes)
            && time_of_day(&self.until_time))
        .then_some(self)
    }
}

fn time_of_day(s: &str) -> bool {
    let Some((h, m)) = s.split_once(':') else {
        return false;
    };
    let two_digits = |p: &str| p.len() == 2 && p.bytes().all(|b| b.is_ascii_digit());
    two_digits(h) && two_digits(m) && h < "24" && m < "60"
}

/// The card with its settings; `ends_at` is set while it is shown.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BreakCard {
    pub settings: BreakSettings,
    pub shown: bool,
    pub ends_at: Option<Timestamp>,
}

/// When a shown card's countdown ends.
#[derive(Debug, Clone, Copy)]
pub enum Countdown {
    Minutes(u32),
    Until(Timestamp),
}

impl BreakCard {
    /// Shows the card counting down from `now`; `None` for 0 or more than a day of minutes, or an end not within
    /// the next day.
    pub(crate) fn show(&mut self, countdown: Countdown, now: Timestamp) -> Option<()> {
        let ends_at = match countdown {
            Countdown::Minutes(m) if (1..=MINUTES_MAX).contains(&m) => {
                now + SignedDuration::from_mins(i64::from(m))
            }
            Countdown::Until(t)
                if t > now && t <= now + SignedDuration::from_mins(MINUTES_MAX.into()) =>
            {
                t
            }
            _ => return None,
        };
        self.shown = true;
        self.ends_at = Some(ends_at);
        Some(())
    }

    pub(crate) fn hide(&mut self) {
        self.shown = false;
        self.ends_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(min: i64) -> Timestamp {
        "2026-11-22T12:00:00Z".parse::<Timestamp>().unwrap() + SignedDuration::from_mins(min)
    }

    #[test]
    fn settings_are_trimmed_and_checked() {
        let ok = BreakSettings {
            text: "  休息  ".into(),
            ..BreakSettings::default()
        };
        assert_eq!(ok.normalized().unwrap().text, "休息");
        let bad = |f: fn(&mut BreakSettings)| {
            let mut s = BreakSettings::default();
            f(&mut s);
            s.normalized().is_none()
        };
        assert!(bad(|s| s.minutes = 0));
        assert!(bad(|s| s.minutes = MINUTES_MAX + 1));
        assert!(bad(|s| s.text = "字".repeat(TEXT_MAX + 1)));
        assert!(bad(|s| s.until_time = "24:00".into()));
        assert!(bad(|s| s.until_time = "9:00".into()));
        assert!(bad(|s| s.until_time = "09:60".into()));
        let empty = BreakSettings {
            end_text: " ".into(),
            ..BreakSettings::default()
        };
        assert_eq!(empty.normalized().unwrap().end_text, "");
    }

    #[test]
    fn minutes_count_from_now_and_until_must_be_within_a_day() {
        let mut card = BreakCard::default();
        card.show(Countdown::Minutes(10), at(0)).unwrap();
        assert!(card.shown);
        assert_eq!(card.ends_at, Some(at(10)));

        card.show(Countdown::Until(at(90)), at(0)).unwrap();
        assert_eq!(card.ends_at, Some(at(90)));

        assert!(card.show(Countdown::Until(at(0)), at(0)).is_none());
        assert!(
            card.show(Countdown::Until(at(24 * 60 + 1)), at(0))
                .is_none()
        );
        assert!(card.show(Countdown::Minutes(0), at(0)).is_none());
        assert_eq!(card.ends_at, Some(at(90)), "a refused show keeps the card");

        card.hide();
        assert!(!card.shown);
        assert_eq!(card.ends_at, None);
    }
}
