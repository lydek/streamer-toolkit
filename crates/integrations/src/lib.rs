//! Adapters for external platforms (Twitch, OBS). Business modules never depend on these SDK details.

use std::time::Duration;

pub mod obs;
pub mod secrets;
pub mod tls;
pub mod twitch;

const MAX_BACKOFF_SECS: u64 = 60;

/// Reconnect delay after `failures` (at least 1) failures in a row: 1 s, doubling, at most 60 s.
pub(crate) fn backoff(failures: u32) -> Duration {
    Duration::from_secs((1u64 << failures.saturating_sub(1).min(6)).min(MAX_BACKOFF_SECS))
}

#[cfg(test)]
mod tests {
    #[test]
    fn backoff_doubles_up_to_a_minute() {
        let secs: Vec<u64> = (1..=9).map(|n| super::backoff(n).as_secs()).collect();
        assert_eq!(secs, [1, 2, 4, 8, 16, 32, 60, 60, 60]);
    }
}
