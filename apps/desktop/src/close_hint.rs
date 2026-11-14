//! State of the hint shown when the admin window's ✕ is pressed (docs/architecture.md §6). Pure logic: `main.rs`
//! emits the event, runs the timer and hides the window, so tests drive the timer by calling `timed_out`.

use std::time::Duration;

use serde::Deserialize;

/// Without `shown` from the admin window within this time, ✕ hides the window directly.
pub const TIMEOUT: Duration = Duration::from_secs(2);

/// What the admin window reports through the `close_hint` command.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Shown,
    Cancel,
    Hide { dont_ask_again: bool },
}

#[derive(Default)]
pub struct CloseHint {
    /// Id of the hint waiting for an answer; a timer for an older hint finds a different id.
    pending: Option<u64>,
    shown: bool,
    last_id: u64,
}

impl CloseHint {
    /// ✕ was pressed. `None`: hide now (asked not to show the hint, or ✕ again while it is open).
    /// `Some(id)`: show the hint and call `timed_out(id)` after `TIMEOUT`.
    pub fn close_requested(&mut self, skip_hint: bool) -> Option<u64> {
        if self.pending.take().is_some() || skip_hint {
            return None;
        }
        self.last_id += 1;
        self.pending = Some(self.last_id);
        self.shown = false;
        Some(self.last_id)
    }

    /// Whether the window must hide because hint `id` was never shown.
    pub fn timed_out(&mut self, id: u64) -> bool {
        if self.pending == Some(id) && !self.shown {
            self.pending = None;
            return true;
        }
        false
    }

    /// Returns whether to hide the window, with the "don't show again" choice.
    pub fn act(&mut self, action: Action) -> Option<bool> {
        match action {
            Action::Shown => {
                self.shown = self.pending.is_some();
                None
            }
            Action::Cancel => {
                self.pending = None;
                None
            }
            Action::Hide { dont_ask_again } => {
                self.pending = None;
                Some(dont_ask_again)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked() -> (CloseHint, u64) {
        let mut hint = CloseHint::default();
        let id = hint.close_requested(false).expect("asks");
        (hint, id)
    }

    #[test]
    fn skip_flag_hides_without_asking() {
        assert_eq!(CloseHint::default().close_requested(true), None);
    }

    #[test]
    fn shown_stops_the_timeout() {
        let (mut hint, id) = asked();
        assert_eq!(hint.act(Action::Shown), None);
        assert!(!hint.timed_out(id));
    }

    #[test]
    fn no_shown_within_the_timeout_hides() {
        let (mut hint, id) = asked();
        assert!(hint.timed_out(id));
        // The next ✕ asks again.
        assert!(hint.close_requested(false).is_some());
    }

    #[test]
    fn cancel_keeps_the_window_and_asks_again_next_time() {
        let (mut hint, id) = asked();
        hint.act(Action::Shown);
        assert_eq!(hint.act(Action::Cancel), None);
        assert!(!hint.timed_out(id));
        assert!(hint.close_requested(false).is_some());
    }

    #[test]
    fn hide_reports_the_dont_ask_again_choice() {
        for choice in [true, false] {
            let (mut hint, id) = asked();
            hint.act(Action::Shown);
            assert_eq!(
                hint.act(Action::Hide {
                    dont_ask_again: choice
                }),
                Some(choice)
            );
            assert!(!hint.timed_out(id));
        }
    }

    #[test]
    fn second_close_while_open_hides() {
        let (mut hint, id) = asked();
        hint.act(Action::Shown);
        assert_eq!(hint.close_requested(false), None);
        assert!(!hint.timed_out(id));
    }

    #[test]
    fn stale_timer_does_not_hide_a_newer_hint() {
        let (mut hint, old) = asked();
        hint.act(Action::Cancel);
        let new = hint.close_requested(false).unwrap();
        assert!(!hint.timed_out(old));
        assert!(hint.timed_out(new));
    }

    #[test]
    fn late_shown_after_timeout_is_ignored() {
        let (mut hint, id) = asked();
        assert!(hint.timed_out(id));
        hint.act(Action::Shown);
        let next = hint.close_requested(false).unwrap();
        assert!(hint.timed_out(next));
    }

    #[test]
    fn actions_parse_from_the_admin_window() {
        let parse = |json| serde_json::from_str::<Action>(json).unwrap();
        assert_eq!(parse(r#""shown""#), Action::Shown);
        assert_eq!(parse(r#""cancel""#), Action::Cancel);
        assert_eq!(
            parse(r#"{"hide":{"dont_ask_again":true}}"#),
            Action::Hide {
                dont_ask_again: true
            }
        );
    }
}
