//! The media library: GIFs and videos a redemption can show on the alerts overlay.

/// The largest file accepted; backups hold every file, so this keeps them restorable.
pub(crate) const MAX_BYTES: usize = 50 * 1024 * 1024;
/// A GIF has no length of its own: it shows this long until changed.
pub(crate) const GIF_MS: u32 = 5_000;
/// The longest a media item may show.
pub(crate) const DURATION_MAX_MS: u32 = 10 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaFormat {
    Gif,
    Webm,
    Mp4,
}

impl MediaFormat {
    const ALL: [Self; 3] = [Self::Gif, Self::Webm, Self::Mp4];

    pub fn extension(self) -> &'static str {
        match self {
            Self::Gif => "gif",
            Self::Webm => "webm",
            Self::Mp4 => "mp4",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Self::Gif => "image/gif",
            Self::Webm => "video/webm",
            Self::Mp4 => "video/mp4",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|f| f.extension().eq_ignore_ascii_case(ext))
    }

    /// The format `file_name`'s extension names, if the content starts the way that format does.
    pub fn detect(file_name: &str, bytes: &[u8]) -> Option<Self> {
        let format = Self::from_extension(file_name.rsplit_once('.')?.1)?;
        let magic = match format {
            Self::Gif => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
            // EBML header.
            Self::Webm => bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]),
            // ISO base media: a box size, then `ftyp`.
            Self::Mp4 => bytes.len() >= 12 && &bytes[4..8] == b"ftyp",
        };
        magic.then_some(format)
    }
}

/// One imported file; it is `media/<id>.<format>` in the data folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaEntry {
    /// UUID v7.
    pub id: String,
    /// Starts as the original file name, which is never used as a path.
    pub title: String,
    pub format: MediaFormat,
    /// How long it shows: 5 s for a GIF until changed; a video's own length once the admin window has read it.
    pub duration_ms: Option<u32>,
}

impl MediaEntry {
    pub(crate) fn file_name(&self) -> String {
        format!("{}.{}", self.id, self.format.extension())
    }
}

/// Where on the alerts overlay a media item shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
    Full,
}

impl Position {
    const ALL: [Self; 6] = [
        Self::TopLeft,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomRight,
        Self::Center,
        Self::Full,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::TopLeft => "top_left",
            Self::TopRight => "top_right",
            Self::BottomLeft => "bottom_left",
            Self::BottomRight => "bottom_right",
            Self::Center => "center",
            Self::Full => "full",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == s)
    }
}

/// What a reward shows when redeemed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaRule {
    pub reward_id: String,
    pub media_id: String,
    pub position: Position,
    /// Percent, 0–100; `None` follows the sound master volume. A GIF has no sound.
    pub volume: Option<u8>,
}

impl MediaRule {
    pub(crate) fn valid(&self) -> bool {
        !self.reward_id.trim().is_empty()
            && self.reward_id.len() <= 64
            && self.volume.is_none_or(|v| v <= 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_and_magic_must_agree() {
        assert_eq!(
            MediaFormat::detect("a.gif", b"GIF89a.."),
            Some(MediaFormat::Gif)
        );
        assert_eq!(
            MediaFormat::detect("A.GIF", b"GIF87a.."),
            Some(MediaFormat::Gif)
        );
        assert_eq!(
            MediaFormat::detect("a.webm", &[0x1A, 0x45, 0xDF, 0xA3, 0x01]),
            Some(MediaFormat::Webm)
        );
        assert_eq!(
            MediaFormat::detect("a.mp4", b"\0\0\0\x18ftypmp42"),
            Some(MediaFormat::Mp4)
        );
        assert_eq!(MediaFormat::detect("a.mp4", b"GIF89a......"), None);
        assert_eq!(MediaFormat::detect("a.gif", b"\0\0\0\x18ftypmp42"), None);
        assert_eq!(MediaFormat::detect("a.mov", b"\0\0\0\x18ftypqt  "), None);
        assert_eq!(MediaFormat::detect("noext", b"GIF89a"), None);
    }

    #[test]
    fn positions_round_trip_and_rules_are_checked() {
        for p in Position::ALL {
            assert_eq!(Position::parse(p.as_str()), Some(p));
        }
        let rule = |reward_id: &str, volume| MediaRule {
            reward_id: reward_id.into(),
            media_id: "m".into(),
            position: Position::Center,
            volume,
        };
        assert!(rule("r", None).valid() && rule("r", Some(100)).valid());
        assert!(!rule(" ", None).valid() && !rule("r", Some(101)).valid());
    }
}
