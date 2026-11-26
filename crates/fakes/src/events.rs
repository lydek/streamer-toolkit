//! EventSub event bodies as Twitch sends them, for the event types the app subscribes to.

use serde_json::{Value, json};

use crate::twitch::{BROADCASTER_ID, BROADCASTER_LOGIN, now};

/// What one fake event says; every field has a default, so `send follow` alone works.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    pub user: String,
    /// Gifted subs, or raid viewers.
    pub count: u32,
    pub message: String,
    pub reward: String,
    pub bits: u32,
    pub tier: String,
    pub months: u32,
    /// A redemption's status for `redeem-update`: `fulfilled` or `canceled`.
    pub status: String,
    /// The redemption or chat message an update or removal is about.
    pub id: String,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            user: "fake_viewer".into(),
            count: 5,
            message: "Hello from the fake Twitch".into(),
            reward: "Check in".into(),
            bits: 100,
            tier: "1000".into(),
            months: 3,
            status: "fulfilled".into(),
            id: String::new(),
        }
    }
}

impl Params {
    /// Reads the control API's JSON body; unknown or mistyped fields keep their defaults.
    pub fn from_json(body: &Value) -> Self {
        let d = Self::default();
        let text = |k: &str, d: String| body[k].as_str().map(str::to_owned).unwrap_or(d);
        let number = |k: &str, d: u32| {
            body[k]
                .as_u64()
                .or_else(|| body[k].as_str().and_then(|s| s.parse().ok()))
                .map_or(d, |n| n.min(u32::MAX as u64) as u32)
        };
        Self {
            user: text("user", d.user),
            count: number("count", d.count),
            message: text("message", d.message),
            reward: text("reward", d.reward),
            bits: number("bits", d.bits),
            tier: text("tier", d.tier),
            months: number("months", d.months),
            status: text("status", d.status),
            id: text("id", d.id),
        }
    }
}

/// The CLI's short names and the Twitch subscription types they stand for.
pub const KINDS: &[(&str, &str)] = &[
    ("follow", "channel.follow"),
    ("sub", "channel.subscribe"),
    ("resub", "channel.subscription.message"),
    ("gift", "channel.subscription.gift"),
    ("cheer", "channel.cheer"),
    ("raid", "channel.raid"),
    ("chat", "channel.chat.message"),
    ("delete", "channel.chat.message_delete"),
    ("clear-user", "channel.chat.clear_user_messages"),
    ("clear", "channel.chat.clear"),
    (
        "redeem",
        "channel.channel_points_custom_reward_redemption.add",
    ),
    (
        "redeem-update",
        "channel.channel_points_custom_reward_redemption.update",
    ),
    ("online", "stream.online"),
    ("offline", "stream.offline"),
];

pub fn twitch_type(kind: &str) -> Option<&'static str> {
    KINDS
        .iter()
        .find(|(k, t)| *k == kind || *t == kind)
        .map(|(_, t)| *t)
}

/// A stable digit ID for a login (the fake Helix answers the same one).
pub fn user_id(login: &str) -> String {
    crate::twitch::id_for(login)
}

fn broadcaster() -> Value {
    json!({"broadcaster_user_id": BROADCASTER_ID, "broadcaster_user_login": BROADCASTER_LOGIN,
           "broadcaster_user_name": BROADCASTER_LOGIN})
}

fn with(mut base: Value, extra: Value) -> Value {
    if let (Some(base), Some(extra)) = (base.as_object_mut(), extra.as_object()) {
        base.extend(extra.clone());
    }
    base
}

fn user(prefix: &str, login: &str) -> Value {
    json!({ format!("{prefix}user_id"): user_id(login), format!("{prefix}user_login"): login,
            format!("{prefix}user_name"): login })
}

/// The event body for a Twitch subscription type; `id` names new messages and redemptions.
pub fn event(twitch_type: &str, p: &Params, id: &str) -> Option<Value> {
    let b = broadcaster();
    let u = user("", &p.user);
    Some(match twitch_type {
        "channel.follow" => with(with(b, u), json!({"followed_at": now()})),
        "channel.subscribe" => with(with(b, u), json!({"tier": p.tier, "is_gift": false})),
        "channel.subscription.message" => with(
            with(b, u),
            json!({"tier": p.tier, "cumulative_months": p.months, "streak_months": null,
                   "duration_months": 1, "message": {"text": p.message, "emotes": []}}),
        ),
        "channel.subscription.gift" => with(
            with(b, u),
            json!({"tier": p.tier, "total": p.count, "cumulative_total": null, "is_anonymous": false}),
        ),
        "channel.cheer" => with(
            with(b, u),
            json!({"is_anonymous": false, "bits": p.bits, "message": p.message}),
        ),
        "channel.raid" => with(
            user("from_broadcaster_", &p.user),
            json!({"to_broadcaster_user_id": BROADCASTER_ID, "to_broadcaster_user_login": BROADCASTER_LOGIN,
                   "to_broadcaster_user_name": BROADCASTER_LOGIN, "viewers": p.count}),
        ),
        "channel.chat.message" => with(
            with(b, user("chatter_", &p.user)),
            json!({"message_id": id, "color": "#1E90FF", "badges": [], "message_type": "text",
                   "message": {"text": p.message, "fragments": [
                       {"type": "text", "text": p.message, "cheermote": null, "emote": null, "mention": null}]},
                   "cheer": null, "reply": null, "channel_points_custom_reward_id": null}),
        ),
        "channel.chat.message_delete" => with(
            with(b, user("target_", &p.user)),
            json!({ "message_id": p.id }),
        ),
        "channel.chat.clear_user_messages" => with(b, user("target_", &p.user)),
        "channel.chat.clear" => b,
        "channel.channel_points_custom_reward_redemption.add"
        | "channel.channel_points_custom_reward_redemption.update" => {
            let add = twitch_type.ends_with(".add");
            let reward_id = format!(
                "fake-reward-{}",
                p.reward.to_lowercase().replace(char::is_whitespace, "-")
            );
            with(
                with(b, u),
                json!({"id": if add { id } else { &p.id }, "user_input": "",
                       "status": if add { "unfulfilled" } else { &p.status },
                       "reward": {"id": reward_id, "title": p.reward, "cost": 100, "prompt": ""},
                       "redeemed_at": now()}),
            )
        }
        "stream.online" => with(b, json!({"id": id, "type": "live", "started_at": now()})),
        "stream.offline" => b,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_an_event() {
        for (kind, twitch) in KINDS {
            assert_eq!(twitch_type(kind), Some(*twitch));
            assert!(event(twitch, &Params::default(), "x").is_some(), "{kind}");
        }
        assert_eq!(twitch_type("nope"), None);
    }

    #[test]
    fn params_come_from_json_with_defaults() {
        let p = Params::from_json(&json!({"user": "a", "count": "20", "bits": 7, "tier": 3}));
        assert_eq!((p.user.as_str(), p.count, p.bits), ("a", 20, 7));
        assert_eq!(p.tier, "1000", "a mistyped field keeps its default");
    }

    #[test]
    fn redemption_updates_name_the_redemption() {
        let p = Params {
            id: "r-1".into(),
            status: "canceled".into(),
            ..Params::default()
        };
        let e = event(
            "channel.channel_points_custom_reward_redemption.update",
            &p,
            "new",
        )
        .unwrap();
        assert_eq!(
            (e["id"].as_str(), e["status"].as_str()),
            (Some("r-1"), Some("canceled"))
        );
        assert_eq!(e["reward"]["id"], "fake-reward-check-in");
    }
}
