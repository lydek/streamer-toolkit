//! What a sound file may be (thin module: simple checks, no aggregates).

/// Largest sound file accepted.
pub const MAX_BYTES: usize = 10 * 1024 * 1024;
/// Longest label kept, in characters.
pub(crate) const LABEL_MAX: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    Mp3,
    Wav,
    Ogg,
}

impl Format {
    const ALL: [Self; 3] = [Self::Mp3, Self::Wav, Self::Ogg];

    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
            Self::Ogg => "ogg",
        }
    }

    /// Fixed per format; the file's content never picks it.
    pub(crate) fn content_type(self) -> &'static str {
        match self {
            Self::Mp3 => "audio/mpeg",
            Self::Wav => "audio/wav",
            Self::Ogg => "audio/ogg",
        }
    }

    pub(crate) fn from_extension(ext: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|f| f.extension().eq_ignore_ascii_case(ext))
    }

    /// The format both the file name's extension and the file's first bytes agree on.
    pub(crate) fn detect(file_name: &str, bytes: &[u8]) -> Option<Self> {
        let format = Self::from_extension(file_name.rsplit_once('.')?.1)?;
        let magic = match format {
            // An ID3 tag, or straight into an MPEG audio frame (11 sync bits).
            Self::Mp3 => {
                bytes.starts_with(b"ID3")
                    || (bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0)
            }
            Self::Wav => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE",
            Self::Ogg => bytes.starts_with(b"OggS"),
        };
        magic.then_some(format)
    }
}

/// The original file name without folders or extension, as a starting label.
pub(crate) fn label_from_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = base.rsplit_once('.').map_or(base, |(stem, _)| stem);
    clean_label(stem).unwrap_or_else(|| "sound".into())
}

/// Trimmed, without control characters and at most [`LABEL_MAX`] characters; `None` when nothing is left.
pub(crate) fn clean_label(label: &str) -> Option<String> {
    let label: String = label
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(LABEL_MAX)
        .collect();
    let label = label.trim_end().to_owned();
    (!label.is_empty()).then_some(label)
}

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
    /// The overlay's own built-in sound.
    BuiltIn,
    Silent,
    /// A library sound at its volume (0–100).
    File {
        id: String,
        volume: u8,
    },
}

/// Whether `slot` names a place a sound can be chosen for: `alert.<kind>`, `checkin` or `checkin.full`.
pub(crate) fn valid_slot(slot: &str) -> bool {
    match slot.split_once('.') {
        Some(("alert", kind)) => ALERT_KINDS.contains(&kind),
        Some(("checkin", "full")) => true,
        Some(_) => false,
        None => slot == "checkin",
    }
}

/// What a slot plays with nothing chosen: alerts keep their built-in sound, everything else is silent.
pub(crate) fn default_cue(slot: &str) -> Cue {
    match slot.starts_with("alert.") {
        true => Cue::BuiltIn,
        false => Cue::Silent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WAV: &[u8] = b"RIFF\x24\0\0\0WAVEfmt ";

    #[test]
    fn extension_and_magic_must_agree() {
        assert_eq!(Format::detect("a.wav", WAV), Some(Format::Wav));
        assert_eq!(Format::detect("A.WAV", WAV), Some(Format::Wav));
        assert_eq!(Format::detect("a.ogg", b"OggS\0\x02"), Some(Format::Ogg));
        assert_eq!(Format::detect("a.mp3", b"ID3\x04\0"), Some(Format::Mp3));
        assert_eq!(
            Format::detect("a.mp3", &[0xFF, 0xFB, 0x90]),
            Some(Format::Mp3)
        );
        assert_eq!(Format::detect("a.mp3", WAV), None, "a WAV named .mp3");
        assert_eq!(Format::detect("a.wav", b"OggS"), None);
        assert_eq!(Format::detect("a.html", b"<html>"), None);
        assert_eq!(Format::detect("a.wav.html", WAV), None);
        assert_eq!(Format::detect("wav", WAV), None, "no extension");
        assert_eq!(Format::detect("a.wav", b"RIFF"), None, "too short");
        assert_eq!(Format::detect("a.mp3", &[]), None);
    }

    #[test]
    fn labels_come_from_the_file_name_only() {
        assert_eq!(label_from_file_name("C:\\x\\..\\bell.mp3"), "bell");
        assert_eq!(label_from_file_name("../../up.down.wav"), "up.down");
        assert_eq!(label_from_file_name("  .ogg"), "sound");
        assert_eq!(
            label_from_file_name("<img src=x onerror=1>.mp3"),
            "<img src=x onerror=1>",
            "kept as text"
        );
        assert_eq!(clean_label(" a\u{7}b "), Some("ab".into()));
        assert_eq!(clean_label("   "), None);
        assert_eq!(
            clean_label(&"字".repeat(150)).unwrap().chars().count(),
            LABEL_MAX
        );
    }

    #[test]
    fn slots_and_their_defaults() {
        for slot in ["alert.follow", "alert.raid", "checkin", "checkin.full"] {
            assert!(valid_slot(slot), "{slot}");
        }
        for slot in [
            "alert",
            "alert.",
            "alert.host",
            "checkin.",
            "checkin.half",
            "x",
            "",
        ] {
            assert!(!valid_slot(slot), "{slot}");
        }
        assert_eq!(default_cue("alert.cheer"), Cue::BuiltIn);
        assert_eq!(default_cue("checkin.full"), Cue::Silent);
    }

    #[test]
    fn volume_is_a_percentage() {
        assert!(valid_volume(0) && valid_volume(100));
        assert!(!valid_volume(101));
    }
}
