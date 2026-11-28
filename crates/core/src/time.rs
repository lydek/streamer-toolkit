//! Instants as the toolkit stores and sends them: UTC, RFC 3339, ending in `Z`.

pub use jiff::{SignedDuration, Timestamp};

/// `2026-11-28T12:00:00.123Z`: what the API, events and the UI see.
pub fn millis(t: Timestamp) -> String {
    t.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// `2026-11-28T12:00:00.123456Z`: what the database stores, so text order is time order.
pub fn micros(t: Timestamp) -> String {
    t.strftime("%Y-%m-%dT%H:%M:%S%.6fZ").to_string()
}

/// `2026-11-28T12:00:00Z`.
pub fn secs(t: Timestamp) -> String {
    t.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// `n` days of 24 hours.
pub const fn days(n: i64) -> SignedDuration {
    SignedDuration::from_hours(n * 24)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instants_keep_the_stored_text_formats() {
        let t: Timestamp = "2026-11-28T20:05:09.1234567+08:00".parse().unwrap();
        assert_eq!(millis(t), "2026-11-28T12:05:09.123Z");
        assert_eq!(micros(t), "2026-11-28T12:05:09.123456Z");
        assert_eq!(secs(t), "2026-11-28T12:05:09Z");
        let whole: Timestamp = "2026-11-28T12:05:09Z".parse().unwrap();
        assert_eq!(millis(whole), "2026-11-28T12:05:09.000Z");
        assert_eq!(micros(whole), "2026-11-28T12:05:09.000000Z");
        assert_eq!(
            whole.strftime("%Y%m%dT%H%M%S%3fZ").to_string(),
            "20261128T120509000Z"
        );
        // Without an offset it is not an instant.
        assert!("2026-11-28T12:05:09".parse::<Timestamp>().is_err());
        assert_eq!(millis(whole + days(1)), "2026-11-29T12:05:09.000Z");
    }
}
