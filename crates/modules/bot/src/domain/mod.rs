//! The chat bot's rules (docs/modules/chat-bot.md): command names, filling in a message's variables, the posting
//! rate, cooldowns, and remembering what the bot posted so it never answers itself.

use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

/// Twitch's limit for one chat message.
pub(crate) const MESSAGE_MAX_CHARS: usize = 500;
pub(crate) const COMMAND_MAX_CHARS: usize = 25;
/// At most `RATE_MAX` messages in any `RATE_WINDOW` (Twitch allows the broadcaster more; this is the safe value).
pub(crate) const RATE_WINDOW: Duration = Duration::from_secs(30);
pub(crate) const RATE_MAX: usize = 20;
/// Automatic messages waiting at most; more are dropped, and so is one that waited longer than `QUEUE_EXPIRY`.
pub(crate) const QUEUE_MAX: usize = 20;
pub(crate) const QUEUE_EXPIRY: Duration = Duration::from_secs(120);
/// How long, and how many of, the IDs of the bot's own messages are kept.
pub(crate) const SENT_KEEP: Duration = Duration::from_secs(300);
pub(crate) const SENT_MAX: usize = 200;

/// A chat command name: one word, with or without its leading `!` (dropped), 1–25 characters; `None` when it
/// cannot be one.
pub(crate) fn command_name(text: &str) -> Option<String> {
    let word = text.trim();
    let word = word.strip_prefix('!').unwrap_or(word);
    let chars = word.chars().count();
    let ok = (1..=COMMAND_MAX_CHARS).contains(&chars)
        && !word.chars().any(|c| c.is_whitespace() || c == '!');
    ok.then(|| word.to_owned())
}

/// What fills a message's variables; `None` leaves that variable as typed.
pub(crate) struct Vars<'a> {
    /// `{觀眾}`: the display name of the viewer who triggered it.
    pub(crate) viewer: Option<&'a str>,
    /// `{數量}`: bits, or a raid's viewers.
    pub(crate) amount: Option<u64>,
    /// `{計數:名稱}`: a counter's value by name.
    pub(crate) counters: &'a dyn Fn(&str) -> Option<i64>,
    /// `{直播時長}`: how long the stream has run; `None` when not live.
    pub(crate) live_for: Option<Duration>,
}

/// `H:MM`.
fn hours_minutes(d: Duration) -> String {
    let minutes = d.as_secs() / 60;
    format!("{}:{:02}", minutes / 60, minutes % 60)
}

/// The message with its variables replaced, trimmed and cut to Twitch's limit; `None` when nothing is left to send.
/// Only fixed replacements: a variable it does not know stays as typed.
pub(crate) fn render(template: &str, vars: &Vars) -> Option<String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            rest = "";
            break;
        };
        let key = &after[..close];
        let value = match key {
            "觀眾" => vars.viewer.map(str::to_owned),
            "數量" => vars.amount.map(|n| n.to_string()),
            "直播時長" => Some(
                vars.live_for
                    .map_or_else(|| "未開播".to_owned(), hours_minutes),
            ),
            _ => key
                .strip_prefix("計數:")
                .and_then(|name| (vars.counters)(name))
                .map(|n| n.to_string()),
        };
        match value {
            Some(v) => out.push_str(&v),
            None => out.push_str(&rest[open..open + 1 + close + 1]),
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    let text = out.trim();
    (!text.is_empty()).then(|| text.chars().take(MESSAGE_MAX_CHARS).collect())
}

/// The messages posted in the last `RATE_WINDOW`, the streamer's own included.
#[derive(Default)]
pub(crate) struct RateWindow {
    sent: VecDeque<Instant>,
}

impl RateWindow {
    pub(crate) fn record(&mut self, at: Instant) {
        self.sent.push_back(at);
    }

    /// How long until one more may go out at `now`; zero when it may now.
    pub(crate) fn wait(&mut self, now: Instant) -> Duration {
        while self
            .sent
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= RATE_WINDOW)
        {
            self.sent.pop_front();
        }
        if self.sent.len() < RATE_MAX {
            return Duration::ZERO;
        }
        (self.sent[0] + RATE_WINDOW).saturating_duration_since(now)
    }
}

/// Twitch's IDs of the bot's own messages, kept for `SENT_KEEP` (at most `SENT_MAX`).
#[derive(Default)]
pub(crate) struct SentMessages {
    ids: VecDeque<(Instant, String)>,
}

impl SentMessages {
    pub(crate) fn remember(&mut self, id: String, at: Instant) {
        self.ids.push_back((at, id));
        while self.ids.len() > SENT_MAX {
            self.ids.pop_front();
        }
    }

    pub(crate) fn contains(&mut self, id: &str, now: Instant) -> bool {
        while self
            .ids
            .front()
            .is_some_and(|(t, _)| now.saturating_duration_since(*t) >= SENT_KEEP)
        {
            self.ids.pop_front();
        }
        self.ids.iter().any(|(_, i)| i == id)
    }
}

/// When each command last replied.
#[derive(Default)]
pub(crate) struct Cooldowns {
    last: HashMap<String, Instant>,
}

impl Cooldowns {
    /// Starts the cooldown of command `id` and says yes, unless it is still cooling down from its last reply.
    pub(crate) fn take(&mut self, id: &str, cooldown: Duration, now: Instant) -> bool {
        if self
            .last
            .get(id)
            .is_some_and(|t| now.saturating_duration_since(*t) < cooldown)
        {
            return false;
        }
        self.last.insert(id.to_owned(), now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars<'a>(counters: &'a dyn Fn(&str) -> Option<i64>) -> Vars<'a> {
        Vars {
            viewer: Some("小魚"),
            amount: Some(100),
            counters,
            live_for: Some(Duration::from_secs(3 * 3600 + 5 * 60 + 59)),
        }
    }

    #[test]
    fn command_names_are_one_word() {
        assert_eq!(command_name(" !discord "), Some("discord".into()));
        assert_eq!(command_name("死亡"), Some("死亡".into()));
        for bad in ["", "!", "a b", "a!b", &"字".repeat(26)] {
            assert_eq!(command_name(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn variables_are_filled_in_and_unknown_ones_stay() {
        let counters = |name: &str| (name == "死亡次數").then_some(7);
        assert_eq!(
            render(
                " 嗨 {觀眾}，{數量} 點，死了 {計數:死亡次數} 次，開台 {直播時長}，{計數:沒有} {歌單網址} {未完",
                &vars(&counters)
            )
            .as_deref(),
            Some("嗨 小魚，100 點，死了 7 次，開台 3:05，{計數:沒有} {歌單網址} {未完")
        );
        let none = |_: &str| None;
        let offline = Vars {
            viewer: None,
            amount: None,
            counters: &none,
            live_for: None,
        };
        assert_eq!(
            render("{觀眾} {數量} {直播時長}", &offline).as_deref(),
            Some("{觀眾} {數量} 未開播")
        );
        assert_eq!(render("   ", &offline), None);
        assert_eq!(
            render(&"字".repeat(600), &offline).unwrap().chars().count(),
            MESSAGE_MAX_CHARS
        );
    }

    #[test]
    fn at_most_twenty_messages_go_out_in_any_thirty_seconds() {
        let start = Instant::now();
        let mut window = RateWindow::default();
        for i in 0..RATE_MAX {
            assert_eq!(window.wait(start), Duration::ZERO);
            window.record(start + Duration::from_secs(i as u64));
        }
        let now = start + Duration::from_secs(20);
        // The first one leaves the window 30 s after it went out.
        assert_eq!(window.wait(now), Duration::from_secs(10));
        assert_eq!(window.wait(start + RATE_WINDOW), Duration::ZERO);
    }

    #[test]
    fn the_bot_knows_its_own_messages_for_five_minutes() {
        let start = Instant::now();
        let mut sent = SentMessages::default();
        sent.remember("m1".into(), start);
        assert!(sent.contains("m1", start + Duration::from_secs(299)));
        assert!(!sent.contains("m2", start));
        assert!(!sent.contains("m1", start + SENT_KEEP));
        for i in 0..=SENT_MAX {
            sent.remember(format!("x{i}"), start);
        }
        assert!(!sent.contains("x0", start));
        assert!(sent.contains(&format!("x{SENT_MAX}"), start));
    }

    #[test]
    fn a_command_cools_down_from_its_last_reply() {
        let start = Instant::now();
        let mut cooldowns = Cooldowns::default();
        let thirty = Duration::from_secs(30);
        assert!(cooldowns.take("c", thirty, start));
        assert!(!cooldowns.take("c", thirty, start + Duration::from_secs(29)));
        assert!(cooldowns.take("other", thirty, start));
        assert!(cooldowns.take("c", thirty, start + thirty));
        assert!(cooldowns.take("none", Duration::ZERO, start));
        assert!(cooldowns.take("none", Duration::ZERO, start));
    }
}
