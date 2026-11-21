//! What a sound file may be (thin module: simple checks, no aggregates).

pub(crate) use toolkit_core::audio::{Format, clean_label};

/// Largest sound file accepted.
pub const MAX_BYTES: usize = 10 * 1024 * 1024;

/// A volume in percent.
pub(crate) fn valid_volume(volume: u8) -> bool {
    volume <= 100
}

/// A slot set to play nothing, rather than its default.
pub(crate) const SILENT: &str = "silent";
const ALERT_KINDS: [&str; 6] = [
    "follow",
    "subscribe",
    "resubscribe",
    "gift",
    "cheer",
    "raid",
];

/// What a slot plays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cue {
    /// The overlay's own built-in sound at a volume (0–100).
    BuiltIn {
        volume: u8,
    },
    Silent,
    /// A library sound at its volume (0–100).
    File {
        id: String,
        volume: u8,
    },
}

/// A volume (0–100) with the master volume (0–100) applied on top.
pub(crate) fn scaled(volume: u8, master: u8) -> u8 {
    (u16::from(volume.min(100)) * u16::from(master.min(100)) / 100) as u8
}

impl Cue {
    /// This cue with the master volume (0–100) applied on top of its own.
    pub(crate) fn at(self, master: u8) -> Self {
        let scale = |volume| scaled(volume, master);
        match self {
            Self::BuiltIn { volume } => Self::BuiltIn {
                volume: scale(volume),
            },
            Self::Silent => Self::Silent,
            Self::File { id, volume } => Self::File {
                id,
                volume: scale(volume),
            },
        }
    }
}

/// Whether `slot` names a place a sound can be chosen for: `alert.<kind>`, `checkin`, `checkin.full` or
/// `reward.<Twitch reward ID>`.
pub(crate) fn valid_slot(slot: &str) -> bool {
    match slot.split_once('.') {
        Some(("alert", kind)) => ALERT_KINDS.contains(&kind),
        Some(("checkin", "full")) => true,
        // Twitch reward IDs are UUIDs; anything close enough is kept, nothing that could be markup or a path.
        Some(("reward", id)) => {
            (1..=64).contains(&id.len())
                && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        }
        Some(_) => false,
        None => slot == "checkin",
    }
}

/// What a slot plays with nothing chosen: alerts keep their built-in sound, everything else is silent.
pub(crate) fn default_cue(slot: &str) -> Cue {
    match slot.starts_with("alert.") {
        true => Cue::BuiltIn { volume: 100 },
        false => Cue::Silent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_and_their_defaults() {
        for slot in [
            "alert.follow",
            "alert.raid",
            "checkin",
            "checkin.full",
            "reward.92af127c-7326-4483-a52b-b0da0be61c01",
        ] {
            assert!(valid_slot(slot), "{slot}");
        }
        for slot in [
            "alert",
            "alert.",
            "alert.host",
            "checkin.",
            "checkin.half",
            "reward.",
            "reward.<b>",
            "reward.a/b",
            &format!("reward.{}", "a".repeat(65)),
            "x",
            "",
        ] {
            assert!(!valid_slot(slot), "{slot}");
        }
        assert_eq!(default_cue("alert.cheer"), Cue::BuiltIn { volume: 100 });
        assert_eq!(default_cue("checkin.full"), Cue::Silent);
        assert_eq!(default_cue("reward.r1"), Cue::Silent);
    }

    #[test]
    fn volume_is_a_percentage() {
        assert!(valid_volume(0) && valid_volume(100));
        assert!(!valid_volume(101));
    }

    #[test]
    fn the_master_volume_scales_every_cue() {
        let file = |volume| Cue::File {
            id: "a".into(),
            volume,
        };
        assert_eq!(file(80).at(50), file(40));
        assert_eq!(file(80).at(100), file(80));
        assert_eq!(
            Cue::BuiltIn { volume: 100 }.at(0),
            Cue::BuiltIn { volume: 0 }
        );
        assert_eq!(Cue::Silent.at(50), Cue::Silent);
        // An edited settings file cannot push past 100%.
        assert_eq!(file(200).at(200), file(100));
    }
}
