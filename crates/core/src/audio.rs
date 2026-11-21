//! What an imported audio file may be, shared by the sound library and the background music library so both check
//! files the same way.

/// Longest label or title kept, in characters.
pub const LABEL_MAX: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Mp3,
    Wav,
    Ogg,
}

impl Format {
    const ALL: [Self; 3] = [Self::Mp3, Self::Wav, Self::Ogg];

    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
            Self::Ogg => "ogg",
        }
    }

    /// Fixed per format; the file's content never picks it.
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Mp3 => "audio/mpeg",
            Self::Wav => "audio/wav",
            Self::Ogg => "audio/ogg",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|f| f.extension().eq_ignore_ascii_case(ext))
    }

    /// The format both the file name's extension and the file's first bytes agree on.
    pub fn detect(file_name: &str, bytes: &[u8]) -> Option<Self> {
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

/// The original file name without folders or extension, as a starting label; `fallback` when nothing is left.
pub fn label_from_file_name(name: &str, fallback: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = base.rsplit_once('.').map_or(base, |(stem, _)| stem);
    clean_label(stem).unwrap_or_else(|| fallback.into())
}

/// Trimmed, without control characters and at most [`LABEL_MAX`] characters; `None` when nothing is left.
pub fn clean_label(label: &str) -> Option<String> {
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
        assert_eq!(label_from_file_name("C:\\x\\..\\bell.mp3", "sound"), "bell");
        assert_eq!(
            label_from_file_name("../../up.down.wav", "sound"),
            "up.down"
        );
        assert_eq!(label_from_file_name("  .ogg", "sound"), "sound");
        assert_eq!(
            label_from_file_name("<img src=x onerror=1>.mp3", "sound"),
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
}
