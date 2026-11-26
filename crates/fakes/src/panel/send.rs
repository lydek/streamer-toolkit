//! The send-event form: which fields each kind needs, and the control body it sends — the same body the
//! `fake-services send` command builds, so the panel and the command cannot drift apart.

use serde_json::{Value, json};

/// The kinds in the form's order, with their labels.
pub const KINDS: [(&str, &str); 14] = [
    ("follow", "追隨"),
    ("sub", "訂閱"),
    ("resub", "續訂"),
    ("gift", "送禮"),
    ("cheer", "小奇點"),
    ("raid", "揪團"),
    ("chat", "聊天"),
    ("delete", "刪除訊息"),
    ("clear-user", "清除此人訊息"),
    ("clear", "清空聊天"),
    ("redeem", "兌換"),
    ("redeem-update", "兌換狀態"),
    ("online", "開播"),
    ("offline", "下播"),
];

/// The quick buttons, two rows of six.
pub const QUICK: [(&str, &str); 12] = [
    ("follow", "追隨"),
    ("sub", "訂閱"),
    ("resub", "續訂"),
    ("gift", "送禮 ×5"),
    ("cheer", "小奇點 100"),
    ("raid", "揪團 5 人"),
    ("chat", "聊天"),
    ("redeem", "兌換"),
    ("online", "開播"),
    ("offline", "下播"),
    ("clear-user", "清除此人訊息"),
    ("clear", "清空聊天"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    User,
    Count,
    Bits,
    Tier,
    Months,
    Message,
    Reward,
    Id,
    Status,
}

impl Field {
    pub const ALL: [Field; 9] = [
        Field::User,
        Field::Count,
        Field::Bits,
        Field::Tier,
        Field::Months,
        Field::Message,
        Field::Reward,
        Field::Id,
        Field::Status,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Field::User => "觀眾",
            Field::Count => "數量",
            Field::Bits => "小奇點",
            Field::Tier => "等級",
            Field::Months => "累計月數",
            Field::Message => "訊息",
            Field::Reward => "獎勵",
            Field::Id => "對象 ID",
            Field::Status => "新狀態",
        }
    }

    /// The control body's key, same as the command's `--<key>`.
    pub fn key(self) -> &'static str {
        match self {
            Field::User => "user",
            Field::Count => "count",
            Field::Bits => "bits",
            Field::Tier => "tier",
            Field::Months => "months",
            Field::Message => "message",
            Field::Reward => "reward",
            Field::Id => "id",
            Field::Status => "status",
        }
    }

    /// Whether `kind` uses this field.
    pub fn used_by(self, kind: &str) -> bool {
        let kinds: &[&str] = match self {
            Field::User => &[
                "follow",
                "sub",
                "resub",
                "gift",
                "cheer",
                "raid",
                "chat",
                "clear-user",
                "redeem",
            ],
            Field::Count => &["gift", "raid"],
            Field::Bits => &["cheer"],
            Field::Tier => &["sub", "resub", "gift"],
            Field::Months => &["resub"],
            Field::Message => &["chat", "resub", "cheer"],
            Field::Reward => &["redeem"],
            Field::Id => &["redeem-update", "delete"],
            Field::Status => &["redeem-update"],
        };
        kinds.contains(&kind)
    }
}

/// A kind in the form's picker, shown by its label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kind(pub &'static str);

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = KINDS
            .iter()
            .find(|(k, _)| *k == self.0)
            .map_or(self.0, |(_, l)| l);
        f.write_str(label)
    }
}

/// A redemption's new status, shown in Chinese.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status(pub &'static str);

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0 == "canceled" {
            "取消"
        } else {
            "完成"
        })
    }
}

pub const STATUSES: [Status; 2] = [Status("fulfilled"), Status("canceled")];
pub const TIERS: [&str; 3] = ["1000", "2000", "3000"];

#[derive(Debug, Clone, PartialEq)]
pub struct Form {
    pub kind: &'static str,
    pub user: String,
    pub count: String,
    pub bits: String,
    pub tier: &'static str,
    pub months: String,
    pub message: String,
    pub reward: &'static str,
    pub id: String,
    pub status: &'static str,
    pub first: bool,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            kind: "follow",
            user: "fake_viewer".into(),
            count: "5".into(),
            bits: "100".into(),
            tier: "1000",
            months: "3".into(),
            message: "早安主播！".into(),
            reward: "Check in",
            id: String::new(),
            status: "fulfilled",
            first: false,
        }
    }
}

impl Form {
    pub fn value(&self, field: Field) -> &str {
        match field {
            Field::User => &self.user,
            Field::Count => &self.count,
            Field::Bits => &self.bits,
            Field::Tier => self.tier,
            Field::Months => &self.months,
            Field::Message => &self.message,
            Field::Reward => self.reward,
            Field::Id => &self.id,
            Field::Status => self.status,
        }
    }

    /// Sets a typed field; the picked ones (tier, reward, status) change through their own setters.
    pub fn set(&mut self, field: Field, value: String) {
        match field {
            Field::User => self.user = value,
            Field::Count => self.count = value,
            Field::Bits => self.bits = value,
            Field::Months => self.months = value,
            Field::Message => self.message = value,
            Field::Id => self.id = value,
            Field::Tier | Field::Reward | Field::Status => {}
        }
    }

    /// The body for the form's kind: only the fields that kind uses, all as text like the command sends.
    pub fn body(&self) -> Value {
        self.body_for(self.kind)
    }

    /// The body for `kind` from the form's values (the quick buttons use the form's viewer, message and reward).
    pub fn body_for(&self, kind: &str) -> Value {
        let mut body = json!({ "kind": kind });
        for field in Field::ALL.into_iter().filter(|f| f.used_by(kind)) {
            // `bits` goes to the event as `bits`; a cheer's count field is separate.
            body[field.key()] = json!(self.value(field));
        }
        if kind == "chat" && self.first {
            body["first"] = json!(true);
        }
        body
    }

    /// The `fake-services send …` words for the same event (the test that keeps the form and the command alike).
    #[cfg(test)]
    pub fn command_words(&self) -> Vec<String> {
        let mut words = vec!["send".to_owned(), self.kind.to_owned()];
        for field in Field::ALL.into_iter().filter(|f| f.used_by(self.kind)) {
            words.push(format!("--{}", field.key()));
            words.push(self.value(field).to_owned());
        }
        if self.kind == "chat" && self.first {
            words.push("--first".into());
        }
        words
    }
}

/// The form kind a delivered activity-log event fills in when clicked: an update for a redemption, a deletion for a
/// chat message.
pub fn pick_kind(event_kind: &str) -> Option<&'static str> {
    match event_kind {
        "channel.channel_points_custom_reward_redemption.add" => Some("redeem-update"),
        "channel.chat.message" => Some("delete"),
        _ => None,
    }
}

/// The notice line after sending or running a scenario, and whether it needs attention.
pub fn notice(result: &Result<Value, String>) -> (String, bool) {
    let result = match result {
        Ok(result) => result,
        Err(e) => return (format!("失敗：{e}"), true),
    };
    let names = |key: &str| {
        result[key]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("、")
            })
            .unwrap_or_default()
    };
    match result["id"].as_str() {
        Some(id) if !names("skipped").is_empty() => {
            (format!("{id}：app 沒訂閱 {}", names("skipped")), true)
        }
        Some(id) => (format!("已送出 {id}"), false),
        None => ("完成".to_owned(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control;

    #[test]
    fn every_kind_sends_what_the_command_sends() {
        for (kind, _) in KINDS {
            for first in [false, true] {
                let form = Form {
                    kind,
                    first,
                    id: "fake-9".into(),
                    ..Form::default()
                };
                let (path, body) = control::command(&form.command_words()).unwrap();
                assert_eq!(path, "/_control/send");
                assert_eq!(form.body(), body, "{kind} first={first}");
            }
        }
    }

    #[test]
    fn each_kind_shows_only_its_fields() {
        let shown = |kind| {
            Field::ALL
                .into_iter()
                .filter(|f| f.used_by(kind))
                .map(Field::label)
                .collect::<Vec<_>>()
        };
        assert_eq!(shown("redeem"), ["觀眾", "獎勵"]);
        assert_eq!(shown("redeem-update"), ["對象 ID", "新狀態"]);
        assert_eq!(shown("clear"), Vec::<&str>::new());
        assert_eq!(shown("resub"), ["觀眾", "等級", "累計月數", "訊息"]);
    }

    #[test]
    fn quick_kinds_and_labels_are_known() {
        for (kind, _) in QUICK {
            assert!(crate::events::twitch_type(kind).is_some(), "{kind}");
        }
        assert_eq!(Kind("clear-user").to_string(), "清除此人訊息");
        assert_eq!(Status("canceled").to_string(), "取消");
    }

    #[test]
    fn picking_a_logged_event_fills_its_update() {
        assert_eq!(
            pick_kind("channel.channel_points_custom_reward_redemption.add"),
            Some("redeem-update")
        );
        assert_eq!(pick_kind("channel.chat.message"), Some("delete"));
        assert_eq!(pick_kind("channel.follow"), None);
    }

    #[test]
    fn notices_flag_failures_and_unsubscribed_events() {
        let sent = json!({"id": "fake-2", "sent": ["channel.follow"], "skipped": []});
        assert_eq!(notice(&Ok(sent)), ("已送出 fake-2".to_owned(), false));
        let skipped = json!({"id": "fake-3", "sent": [], "skipped": ["channel.raid"]});
        assert_eq!(
            notice(&Ok(skipped)),
            ("fake-3：app 沒訂閱 channel.raid".to_owned(), true)
        );
        assert_eq!(
            notice(&Ok(json!({"done": "redemption-flood"}))),
            ("完成".to_owned(), false)
        );
        assert!(notice(&Err("unknown event kind x".into())).1);
    }
}
