//! An activity record and how long records are kept.

use chrono::{DateTime, TimeDelta, Utc};
use toolkit_core::{
    events::{Alert, AlertKind},
    settings::RecordRetention,
};

/// What `alert` stores: every field it has, the session it happened in, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Record {
    pub(crate) twitch_message_id: String,
    pub(crate) kind: &'static str,
    pub(crate) user_id: Option<String>,
    pub(crate) user_login: Option<String>,
    pub(crate) user_name: Option<String>,
    pub(crate) tier: Option<u8>,
    pub(crate) months: Option<u32>,
    pub(crate) count: Option<u32>,
    pub(crate) bits: Option<u32>,
    pub(crate) viewers: Option<u32>,
    pub(crate) gifted: bool,
    pub(crate) message: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) occurred_at: DateTime<Utc>,
}

impl Record {
    pub(crate) fn from_alert(alert: Alert, session_id: Option<String>, now: DateTime<Utc>) -> Self {
        let text = |m: String| (!m.is_empty()).then_some(m);
        let mut r = Record {
            twitch_message_id: alert.id,
            kind: "follow",
            user_id: alert.user_id,
            user_login: alert.user_login,
            user_name: alert.user_name,
            tier: None,
            months: None,
            count: None,
            bits: None,
            viewers: None,
            gifted: false,
            message: None,
            session_id,
            // Twitch always sends RFC 3339; if not, it happened about now.
            occurred_at: DateTime::parse_from_rfc3339(&alert.occurred_at)
                .map_or(now, |t| t.to_utc()),
        };
        match alert.kind {
            AlertKind::Follow => {}
            AlertKind::Subscribe { tier, gifted } => {
                r.kind = "subscribe";
                r.tier = Some(tier);
                r.gifted = gifted;
            }
            AlertKind::Resubscribe {
                tier,
                months,
                message,
            } => {
                r.kind = "resubscribe";
                r.tier = Some(tier);
                r.months = Some(months);
                r.message = text(message);
            }
            AlertKind::Gift { tier, count } => {
                r.kind = "gift";
                r.tier = Some(tier);
                r.count = Some(count);
            }
            AlertKind::Cheer { bits, message } => {
                r.kind = "cheer";
                r.bits = Some(bits);
                r.message = text(message);
            }
            AlertKind::Raid { viewers } => {
                r.kind = "raid";
                r.viewers = Some(viewers);
            }
        }
        r
    }
}

/// Records from before this time are past `retention`; `None` keeps them all.
pub(crate) fn cutoff(retention: RecordRetention, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    match retention {
        RecordRetention::Days(days) => Some(now - TimeDelta::days(days.into())),
        RecordRetention::Forever => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().to_utc()
    }

    #[test]
    fn each_kind_keeps_its_fields() {
        let alert = |kind| Alert {
            id: "m1".into(),
            user_id: Some("7".into()),
            user_login: Some("fan".into()),
            user_name: Some("Fan".into()),
            kind,
            occurred_at: "2026-11-19T10:00:00Z".into(),
        };
        let now = at("2026-11-19T11:00:00Z");
        let cheer = Record::from_alert(
            alert(AlertKind::Cheer {
                bits: 100,
                message: String::new(),
            }),
            Some("s1".into()),
            now,
        );
        assert_eq!(
            (
                cheer.kind,
                cheer.bits,
                cheer.message.as_deref(),
                cheer.session_id.as_deref()
            ),
            ("cheer", Some(100), None, Some("s1"))
        );
        assert_eq!(cheer.occurred_at, at("2026-11-19T10:00:00Z"));
        let gifted = Record::from_alert(
            alert(AlertKind::Subscribe {
                tier: 2,
                gifted: true,
            }),
            None,
            now,
        );
        assert_eq!(
            (gifted.kind, gifted.tier, gifted.gifted),
            ("subscribe", Some(2), true)
        );
        let resub = Record::from_alert(
            alert(AlertKind::Resubscribe {
                tier: 1,
                months: 12,
                message: "hi".into(),
            }),
            None,
            now,
        );
        assert_eq!(
            (resub.months, resub.message.as_deref()),
            (Some(12), Some("hi"))
        );
    }

    #[test]
    fn the_cutoff_follows_the_retention() {
        let now = at("2026-11-19T10:00:00Z");
        assert_eq!(
            cutoff(RecordRetention::Days(30), now),
            Some(at("2026-10-20T10:00:00Z"))
        );
        assert_eq!(cutoff(RecordRetention::Forever, now), None);
    }
}
