//! The built-in overlays' look, chosen apart from the admin UI's theme.

/// A theme for the built-in overlays; dark is how they look without one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    Dark,
    Cream,
    Contrast,
}

impl Theme {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Cream => "cream",
            Self::Contrast => "contrast",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "dark" => Some(Self::Dark),
            "cream" => Some(Self::Cream),
            "contrast" => Some(Self::Contrast),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Appearance {
    pub theme: Theme,
    /// Percent of each overlay's own text size; OBS source sizes stay as they are.
    pub text_size: u16,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            text_size: 100,
        }
    }
}

impl Appearance {
    /// 50–200 % in steps of 10.
    pub(crate) fn valid(self) -> bool {
        (50..=200).contains(&self.text_size) && self.text_size.is_multiple_of(10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_size_range_and_steps() {
        let size = |text_size| Appearance {
            text_size,
            ..Appearance::default()
        };
        assert!(size(50).valid() && size(100).valid() && size(200).valid());
        assert!(!size(40).valid() && !size(210).valid() && !size(105).valid());
    }

    #[test]
    fn themes_round_trip() {
        for t in [Theme::Dark, Theme::Cream, Theme::Contrast] {
            assert_eq!(Theme::parse(t.as_str()), Some(t));
        }
        assert_eq!(Theme::parse("light"), None);
    }
}
