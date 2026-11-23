//! On-stream warnings: which ones are on, and what the toolkit last saw of the chat overlay, Twitch and the mic.

use chrono::{DateTime, Utc};
use toolkit_core::streaming::Input;

/// The longest wait before a muted mic warns: an hour.
pub(crate) const MIC_SECONDS_MAX: u32 = 3600;
/// OBS input names are short; this only bounds what is stored.
const INPUT_NAME_MAX: usize = 256;

/// Which warnings the broadcaster turned on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarningSettings {
    pub chat_overlay: bool,
    pub twitch: bool,
    pub mic: bool,
    /// How long the mic must stay muted before it warns.
    pub mic_seconds: u32,
    /// The OBS audio input to watch; `None`: the mic is not checked.
    pub mic_input: Option<String>,
}

impl Default for WarningSettings {
    fn default() -> Self {
        Self {
            chat_overlay: true,
            twitch: false,
            mic: true,
            mic_seconds: 30,
            mic_input: None,
        }
    }
}

impl WarningSettings {
    /// 1 to [`MIC_SECONDS_MAX`] seconds; a blank input name means none.
    pub(crate) fn normalized(mut self) -> Option<Self> {
        self.mic_input = self
            .mic_input
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty());
        let short = self
            .mic_input
            .as_ref()
            .is_none_or(|n| n.chars().count() <= INPUT_NAME_MAX);
        ((1..=MIC_SECONDS_MAX).contains(&self.mic_seconds) && short).then_some(self)
    }
}

/// What the host saw just now.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Conditions {
    /// A chat overlay is open in OBS.
    pub chat_overlay_open: bool,
    pub twitch_connected: bool,
    /// OBS's audio inputs; `None` while OBS is not connected, when no input can be said to be gone.
    pub inputs: Option<Vec<Input>>,
}

/// The warnings' state as of the last conditions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Warnings {
    pub chat_overlay_down: bool,
    pub twitch_down: bool,
    /// Since when the watched mic has been muted; the warning shows `mic_seconds` later.
    pub mic_muted_since: Option<DateTime<Utc>>,
    /// The watched input is no longer in OBS: pick another.
    pub mic_missing: bool,
}

impl Warnings {
    /// The state after seeing `c` at `now`; a mic still muted keeps the time it was first seen muted.
    pub(crate) fn next(
        &self,
        settings: &WarningSettings,
        c: &Conditions,
        now: DateTime<Utc>,
    ) -> Self {
        let watched = settings.mic_input.as_deref().and_then(|name| {
            c.inputs
                .as_ref()
                .map(|inputs| inputs.iter().find(|i| i.name == name))
        });
        let muted = matches!(watched, Some(Some(i)) if i.muted);
        Self {
            chat_overlay_down: !c.chat_overlay_open,
            twitch_down: !c.twitch_connected,
            mic_muted_since: muted.then(|| self.mic_muted_since.unwrap_or(now)),
            mic_missing: matches!(watched, Some(None)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + s, 0).unwrap()
    }

    fn mic(muted: bool) -> Conditions {
        Conditions {
            chat_overlay_open: true,
            twitch_connected: true,
            inputs: Some(vec![Input {
                name: "Mic".into(),
                muted,
                ..Default::default()
            }]),
        }
    }

    fn watching() -> WarningSettings {
        WarningSettings {
            mic_input: Some("Mic".into()),
            ..WarningSettings::default()
        }
    }

    #[test]
    fn chat_overlay_and_twitch_follow_what_was_seen() {
        let w =
            Warnings::default().next(&WarningSettings::default(), &Conditions::default(), at(0));
        assert!(w.chat_overlay_down && w.twitch_down);
        let w = w.next(&WarningSettings::default(), &mic(false), at(1));
        assert!(!w.chat_overlay_down && !w.twitch_down);
    }

    #[test]
    fn a_muted_mic_keeps_the_time_it_was_first_seen_muted() {
        let s = watching();
        let w = Warnings::default().next(&s, &mic(true), at(0));
        assert_eq!(w.mic_muted_since, Some(at(0)));
        let w = w.next(&s, &mic(true), at(10));
        assert_eq!(w.mic_muted_since, Some(at(0)));
        let w = w.next(&s, &mic(false), at(11));
        assert_eq!(w.mic_muted_since, None);
    }

    #[test]
    fn no_input_picked_checks_nothing_and_a_deleted_one_asks_for_another() {
        let w = Warnings::default().next(&WarningSettings::default(), &mic(true), at(0));
        assert_eq!(w.mic_muted_since, None);
        assert!(!w.mic_missing);

        let gone = Conditions {
            inputs: Some(vec![]),
            ..mic(true)
        };
        let w = Warnings::default().next(&watching(), &gone, at(0));
        assert!(w.mic_missing);
        assert_eq!(w.mic_muted_since, None);

        let offline = Conditions {
            inputs: None,
            ..mic(true)
        };
        let w = w.next(&watching(), &offline, at(1));
        assert!(
            !w.mic_missing,
            "OBS not connected says nothing about the input"
        );
    }

    #[test]
    fn settings_are_checked() {
        let s = WarningSettings {
            mic_input: Some("  ".into()),
            ..WarningSettings::default()
        };
        assert_eq!(s.normalized().unwrap().mic_input, None);
        for seconds in [0, MIC_SECONDS_MAX + 1] {
            let s = WarningSettings {
                mic_seconds: seconds,
                ..WarningSettings::default()
            };
            assert!(s.normalized().is_none());
        }
    }
}
