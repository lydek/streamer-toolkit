//! What a counter is (thin module: a struct and simple checks).

pub mod marathon;

pub(crate) use toolkit_core::audio::clean_label as clean_name;

/// The most counters kept.
pub(crate) const MAX_COUNTERS: usize = 100;

/// The longest command name, in characters (as the check-in command).
const COMMAND_MAX_CHARS: usize = 25;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counter {
    /// UUID v7.
    pub id: String,
    pub name: String,
    pub value: i64,
    pub per_stream: bool,
    /// The stream session it was last reset for; `None` before any.
    pub reset_session: Option<String>,
}

/// A count kept per viewer, e.g. "who guessed right".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerCounter {
    /// UUID v7.
    pub id: String,
    pub name: String,
}

/// One viewer's count, with the names the viewer directory has for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerCount {
    pub twitch_user_id: String,
    pub login: String,
    pub display_name: String,
    pub value: i64,
}

/// A chat command name: one word, with or without its leading `!` (dropped), at most 25 characters. An empty one
/// means no chat command; `None` when it cannot be one.
pub(crate) fn command_name(text: &str) -> Option<String> {
    let word = text.trim();
    let word = word.strip_prefix('!').unwrap_or(word);
    let ok = word.chars().count() <= COMMAND_MAX_CHARS
        && !word.chars().any(|c| c.is_whitespace() || c == '!');
    ok.then(|| word.to_owned())
}

/// The counter's key in the shared command table.
pub(crate) fn command_key(id: &str) -> String {
    format!("counter.{id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_names_are_one_word() {
        assert_eq!(command_name(" !死亡 "), Some("死亡".into()));
        assert_eq!(command_name(""), Some(String::new()), "no command");
        assert_eq!(command_name("two words"), None);
        assert_eq!(command_name("!!x"), None);
        assert_eq!(command_name(&"x".repeat(26)), None);
    }
}
