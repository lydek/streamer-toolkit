//! The stamp card's look: built-in presets, colors, images, layout, texts with variables and the entry animation.

use toolkit_core::settings::CardStyle;

/// The longest title or description, after trimming.
pub(crate) const TEXT_MAX: usize = 100;
/// The most slots per row.
pub(crate) const PER_ROW_MAX: u32 = 20;
const PRESETS: [&str; 2] = ["simple", "journal"];
const ANIMATIONS: [&str; 4] = ["fade", "slide", "pop", "none"];

/// Image formats a card accepts (never SVG, which can carry script).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageFormat {
    Png,
    Jpg,
    Gif,
    Webp,
}

impl ImageFormat {
    const ALL: [Self; 4] = [Self::Png, Self::Jpg, Self::Gif, Self::Webp];

    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
        }
    }

    pub(crate) fn content_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
        }
    }

    fn from_extension(ext: &str) -> Option<Self> {
        let ext = ext.to_ascii_lowercase();
        let ext = if ext == "jpeg" { "jpg" } else { ext.as_str() };
        Self::ALL.into_iter().find(|f| f.extension() == ext)
    }

    /// The format `file_name`'s extension names, if the content starts the way that format does.
    pub(crate) fn detect(file_name: &str, bytes: &[u8]) -> Option<Self> {
        let format = Self::from_extension(file_name.rsplit_once('.')?.1)?;
        let magic = match format {
            Self::Png => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            Self::Jpg => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
            Self::Gif => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
            Self::Webp => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        };
        magic.then_some(format)
    }

    /// A stored image's file name: a UUID and one of the extensions; anything else is never served.
    pub(crate) fn of_file(name: &str) -> Option<Self> {
        let (stem, ext) = name.split_once('.')?;
        let uuid = stem.len() == 36
            && stem
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'-');
        if !uuid {
            return None;
        }
        Self::ALL.into_iter().find(|f| f.extension() == ext)
    }
}

fn color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// The style trimmed and checked: a known preset and animation, `#RRGGBB` colors, 1 to [`PER_ROW_MAX`] per row,
/// texts of at most [`TEXT_MAX`] characters, and image names the toolkit gave.
pub(crate) fn checked(mut s: CardStyle) -> Option<CardStyle> {
    s.title = s.title.trim().to_owned();
    s.description = s.description.trim().to_owned();
    let short = |t: &str| t.chars().count() <= TEXT_MAX;
    let image = |i: &Option<String>| {
        i.as_deref()
            .is_none_or(|n| ImageFormat::of_file(n).is_some())
    };
    (PRESETS.contains(&s.preset.as_str())
        && ANIMATIONS.contains(&s.animation.as_str())
        && color(&s.background_color)
        && color(&s.stamp_color)
        && (1..=PER_ROW_MAX).contains(&s.per_row)
        && short(&s.title)
        && short(&s.description)
        && image(&s.background_image)
        && image(&s.stamp_image))
    .then_some(s)
}

/// `text` with `#觀眾#`, `#章數#`, `#次數#` and `#格數#` filled in. The result is raw text: the overlay shows it as
/// text, never as HTML.
pub(crate) fn fill(text: &str, viewer: &str, stamps: u32, total: u64, slots: u32) -> String {
    text.replace("#章數#", &stamps.to_string())
        .replace("#次數#", &total.to_string())
        .replace("#格數#", &slots.to_string())
        // Last, so a viewer name holding a variable is never filled in.
        .replace("#觀眾#", viewer)
}

#[cfg(test)]
mod tests {
    use super::*;

    const UUID: &str = "0190a1b2-0000-7000-8000-000000000001";

    #[test]
    fn images_need_matching_extension_and_magic_bytes() {
        assert_eq!(
            ImageFormat::detect("a.png", b"\x89PNG\r\n\x1a\n.."),
            Some(ImageFormat::Png)
        );
        assert_eq!(
            ImageFormat::detect("a.JPEG", &[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(ImageFormat::Jpg)
        );
        assert_eq!(
            ImageFormat::detect("a.gif", b"GIF89a"),
            Some(ImageFormat::Gif)
        );
        assert_eq!(
            ImageFormat::detect("a.webp", b"RIFF\0\0\0\0WEBPVP8 "),
            Some(ImageFormat::Webp)
        );
        assert_eq!(ImageFormat::detect("a.png", b"GIF89a"), None);
        assert_eq!(ImageFormat::detect("a.svg", b"<svg"), None);
        assert_eq!(
            ImageFormat::of_file(&format!("{UUID}.png")),
            Some(ImageFormat::Png)
        );
        assert_eq!(ImageFormat::of_file("../settings.json"), None);
        assert_eq!(ImageFormat::of_file(&format!("{UUID}.svg")), None);
    }

    #[test]
    fn styles_are_checked() {
        assert!(checked(CardStyle::default()).is_some());
        let bad = |f: fn(&mut CardStyle)| {
            let mut s = CardStyle::default();
            f(&mut s);
            checked(s).is_none()
        };
        assert!(bad(|s| s.preset = "fancy".into()));
        assert!(bad(|s| s.animation = "spin".into()));
        assert!(bad(|s| s.background_color = "red".into()));
        assert!(bad(|s| s.stamp_color = "#12345".into()));
        assert!(bad(|s| s.per_row = 0));
        assert!(bad(|s| s.per_row = PER_ROW_MAX + 1));
        assert!(bad(|s| s.title = "字".repeat(TEXT_MAX + 1)));
        assert!(bad(|s| s.background_image = Some("../x.png".into())));
        let ok = CardStyle {
            stamp_image: Some(format!("{UUID}.webp")),
            ..CardStyle::default()
        };
        assert!(checked(ok).is_some());
    }

    #[test]
    fn variables_are_filled_but_never_from_the_viewer_name() {
        assert_eq!(
            fill("#觀眾# #章數#/#格數# 累計 #次數#", "Ann", 3, 13, 10),
            "Ann 3/10 累計 13"
        );
        assert_eq!(fill("#觀眾#", "#次數#", 1, 9, 10), "#次數#");
        assert_eq!(fill("#觀眾#", "<b>Ann</b>", 1, 1, 10), "<b>Ann</b>");
    }
}
