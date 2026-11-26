//! The fault form: the `fault` command's words, so the panel sets faults exactly as the command does.

use serde_json::Value;

/// The connection keys, offered even before the app has made a call.
pub const CONNECT_KEYS: [&str; 3] = ["ws-connect eventsub", "ws-connect irc", "ws-connect obs"];

#[derive(Debug, Clone, PartialEq)]
pub struct FaultForm {
    pub key: String,
    pub status: String,
    pub delay_ms: String,
    pub for_s: String,
    pub close: bool,
}

impl Default for FaultForm {
    fn default() -> Self {
        Self {
            key: String::new(),
            status: "503".into(),
            delay_ms: String::new(),
            for_s: "60".into(),
            close: false,
        }
    }
}

impl FaultForm {
    /// `fault <key> [--status …] [--delay …ms] [--for …s] [--close]`; empty fields are left out.
    pub fn words(&self) -> Vec<String> {
        let mut words = vec!["fault".to_owned(), self.key.trim().to_owned()];
        let mut flag = |name: &str, value: &str, unit: &str| {
            if !value.trim().is_empty() {
                words.push(name.to_owned());
                words.push(format!("{}{unit}", value.trim()));
            }
        };
        flag("--status", &self.status, "");
        flag("--delay", &self.delay_ms, "ms");
        flag("--for", &self.for_s, "s");
        if self.close {
            words.push("--close".into());
        }
        words
    }
}

/// Call keys to pick from: the ones in the stats report, then the connection keys.
pub fn suggestions(report: &Value) -> Vec<String> {
    let mut keys: Vec<String> = report["calls"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["key"].as_str().map(str::to_owned))
        .collect();
    for key in CONNECT_KEYS {
        if !keys.iter().any(|k| k == key) {
            keys.push(key.to_owned());
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control;
    use serde_json::json;

    #[test]
    fn the_form_builds_the_fault_command() {
        let form = FaultForm {
            key: "GET /helix/users".into(),
            delay_ms: "500".into(),
            close: true,
            ..FaultForm::default()
        };
        let (path, body) = control::command(&form.words()).unwrap();
        assert_eq!(path, "/_control/fault");
        assert_eq!(
            body,
            json!({"key": "GET /helix/users", "status": "503", "delay_ms": 500, "for_ms": 60_000, "close": true})
        );
        let bare = FaultForm {
            key: "ws-connect obs".into(),
            status: String::new(),
            for_s: String::new(),
            ..form
        };
        assert_eq!(
            bare.words(),
            ["fault", "ws-connect obs", "--delay", "500ms", "--close"]
        );
    }

    #[test]
    fn suggestions_list_seen_calls_then_connections() {
        let report = json!({"calls": [{"key": "GET /helix/users"}, {"key": "ws-connect obs"}]});
        assert_eq!(
            suggestions(&report),
            [
                "GET /helix/users",
                "ws-connect obs",
                "ws-connect eventsub",
                "ws-connect irc"
            ]
        );
    }
}
