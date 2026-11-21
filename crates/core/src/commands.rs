//! Chat commands: one table for every feature's commands (`chat_commands` in `settings.json`), and one check for
//! who may use them. Each module declares its commands; the host composes the declarations, so a name can belong to
//! only one of them.

use std::{
    collections::BTreeMap,
    io,
    path::PathBuf,
    sync::{RwLock, RwLockWriteGuard},
};

use serde::{Deserialize, Serialize};

use crate::{
    events::{ChatBadge, ChatMessage},
    settings::{self, Settings},
};

/// Who may use a chat command. Each level also lets in the ones above it (subscriber, then VIP, then moderator),
/// and the broadcaster may always use every command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Allowed {
    #[default]
    Everyone,
    Subscriber,
    Vip,
    Moderator,
}

impl Allowed {
    /// Whether the sender of `message` may use a command set to this.
    pub fn permits(self, message: &ChatMessage) -> bool {
        let has = |b: ChatBadge| message.badges.contains(&b);
        if message.user_id == message.broadcaster_id || has(ChatBadge::Broadcaster) {
            return true;
        }
        match self {
            Self::Everyone => true,
            Self::Subscriber => {
                has(ChatBadge::Subscriber) || has(ChatBadge::Vip) || has(ChatBadge::Moderator)
            }
            Self::Vip => has(ChatBadge::Vip) || has(ChatBadge::Moderator),
            Self::Moderator => has(ChatBadge::Moderator),
        }
    }
}

/// One command as saved: the name viewers type without its `!`, who may use it, and whose it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandEntry {
    pub name: String,
    #[serde(default)]
    pub allowed: Allowed,
    /// The module it belongs to.
    #[serde(default)]
    pub module: String,
    /// What the broadcaster calls it (e.g. a counter's name), shown when another command wants the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl CommandEntry {
    /// Whether `message` starts with `!<name>`, ignoring case.
    pub fn matches(&self, message: &str) -> bool {
        message
            .split_whitespace()
            .next()
            .and_then(|w| w.strip_prefix('!'))
            .is_some_and(|w| !self.name.is_empty() && w.to_lowercase() == self.name.to_lowercase())
    }

    /// How a clash names it: its label, or `!<name>`.
    fn shown(&self) -> String {
        self.label
            .clone()
            .unwrap_or_else(|| format!("!{}", self.name))
    }
}

/// A command a feature declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Declared {
    /// Its key in the table, unique across features.
    pub key: &'static str,
    /// The module it belongs to.
    pub module: &'static str,
    /// The name it has until the broadcaster changes it.
    pub default_name: &'static str,
}

/// The admin API's answer to `commands.taken` (409): which command already has the name.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CommandTakenBody {
    /// `commands.taken`.
    pub code: String,
    /// The module whose command has it.
    pub module: String,
    /// How to name that command: what the broadcaster calls it (e.g. a counter's name), or `!<name>`.
    pub label: String,
}

impl CommandTakenBody {
    pub fn new(module: String, label: String) -> Self {
        Self {
            code: "commands.taken".into(),
            module,
            label,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    /// Another command already has the name (ignoring case); `label` is how to name that command.
    #[error("commands.taken")]
    Taken { module: String, label: String },
    #[error("settings.save_failed")]
    SaveFailed(#[from] io::Error),
}

pub struct CommandTable {
    data_dir: PathBuf,
    declared: Vec<Declared>,
    /// What `settings.json` holds, so chat never waits on the file.
    saved: RwLock<BTreeMap<String, CommandEntry>>,
}

impl CommandTable {
    /// The table in `data_dir`, for the commands every module declared and those saved since (e.g. counters').
    pub fn load(data_dir: PathBuf, declared: Vec<Declared>) -> Self {
        let mut saved = settings::load(&data_dir).chat_commands;
        // Saved before entries knew their module.
        for d in &declared {
            if let Some(e) = saved.get_mut(d.key).filter(|e| e.module.is_empty()) {
                e.module = d.module.into();
            }
        }
        Self {
            data_dir,
            declared,
            saved: RwLock::new(saved),
        }
    }

    fn write(&self) -> RwLockWriteGuard<'_, BTreeMap<String, CommandEntry>> {
        self.saved.write().unwrap_or_else(|e| e.into_inner())
    }

    /// The command saved for `key`, if any was ever saved.
    pub fn saved(&self, key: &str) -> Option<CommandEntry> {
        self.saved
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .cloned()
    }

    /// The command for `key`: as saved, or its declared default for everyone (empty for an unknown key).
    pub fn get(&self, key: &str) -> CommandEntry {
        self.saved(key).unwrap_or_else(|| {
            let declared = self.declared.iter().find(|d| d.key == key);
            CommandEntry {
                name: declared.map_or("", |d| d.default_name).to_owned(),
                allowed: Allowed::Everyone,
                module: declared.map_or("", |d| d.module).to_owned(),
                label: None,
            }
        })
    }

    /// Saves `entry` for `key`, together with `also` in the same write of `settings.json`, unless another command
    /// (saved, or declared and never saved) has the same name, ignoring case. Nothing is saved when it fails.
    pub fn set(
        &self,
        key: &str,
        entry: CommandEntry,
        also: impl FnOnce(&mut Settings),
    ) -> Result<(), CommandError> {
        let mut saved = self.write();
        let name = entry.name.to_lowercase();
        let unsaved = self
            .declared
            .iter()
            .filter(|d| d.key != key && !saved.contains_key(d.key))
            .map(|d| CommandEntry {
                name: d.default_name.into(),
                allowed: Allowed::Everyone,
                module: d.module.into(),
                label: None,
            });
        let mut others = saved
            .iter()
            .filter(|(k, _)| k.as_str() != key)
            .map(|(_, e)| e.clone())
            .chain(unsaved);
        if let Some(other) = others.find(|e| e.name.to_lowercase() == name) {
            return Err(CommandError::Taken {
                module: other.module.clone(),
                label: other.shown(),
            });
        }
        settings::update(&self.data_dir, |s| {
            s.chat_commands.insert(key.to_owned(), entry.clone());
            also(s);
        })?;
        saved.insert(key.to_owned(), entry);
        Ok(())
    }

    /// Removes the command for `key` (e.g. a deleted counter's).
    pub fn remove(&self, key: &str) -> Result<(), CommandError> {
        let mut saved = self.write();
        settings::update(&self.data_dir, |s| {
            s.chat_commands.remove(key);
        })?;
        saved.remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Fragment;

    fn message(user_id: &str, badges: &[ChatBadge]) -> ChatMessage {
        ChatMessage {
            id: "m".into(),
            broadcaster_id: "1".into(),
            user_id: user_id.into(),
            user_login: "v".into(),
            user_name: "V".into(),
            color: None,
            text: "!x".into(),
            fragments: vec![Fragment::Text("!x".into())],
            badges: badges.to_vec(),
            highlighted: false,
            bits: 0,
            reply: None,
            twitch_badges: Vec::new(),
            shared_from: None,
            sent_at: "2026-11-21T10:00:00Z".into(),
        }
    }

    #[test]
    fn who_may_use_a_command() {
        use Allowed::*;
        use ChatBadge as B;
        let viewer = message("7", &[]);
        let sub = message("7", &[B::Subscriber]);
        let vip = message("7", &[B::Vip]);
        let moderator = message("7", &[B::Moderator]);
        let broadcaster = message("1", &[]);
        let broadcaster_badge = message("1", &[B::Broadcaster]);
        // Rows: everyone, subscriber, VIP, moderator, broadcaster (by ID and by badge).
        let senders = [
            &viewer,
            &sub,
            &vip,
            &moderator,
            &broadcaster,
            &broadcaster_badge,
        ];
        for (allowed, expected) in [
            (Everyone, [true, true, true, true, true, true]),
            (Subscriber, [false, true, true, true, true, true]),
            (Vip, [false, false, true, true, true, true]),
            (Moderator, [false, false, false, true, true, true]),
        ] {
            let got: Vec<bool> = senders.iter().map(|m| allowed.permits(m)).collect();
            assert_eq!(got, expected, "{allowed:?}");
        }
    }

    const DECLARED: [Declared; 2] = [
        Declared {
            key: "checkin",
            module: "checkin",
            default_name: "打卡",
        },
        Declared {
            key: "counter.deaths",
            module: "counter",
            default_name: "Deaths",
        },
    ];

    #[test]
    fn a_name_belongs_to_one_command_ignoring_case() {
        let dir = std::env::temp_dir().join(format!("st-commands-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let table = CommandTable::load(dir.clone(), DECLARED.to_vec());
        assert_eq!(table.get("checkin").name, "打卡", "the default until saved");
        assert_eq!(table.get("checkin").module, "checkin");
        assert_eq!(table.saved("checkin"), None);

        let entry = |name: &str, module: &str, label: Option<&str>| CommandEntry {
            name: name.into(),
            allowed: Allowed::Subscriber,
            module: module.into(),
            label: label.map(Into::into),
        };
        let checkin = |name: &str| entry(name, "checkin", None);
        match table.set("checkin", checkin("deaths"), |_| {}) {
            Err(CommandError::Taken { module, label }) => {
                assert_eq!((module.as_str(), label.as_str()), ("counter", "!Deaths"))
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            settings::load(&dir).chat_commands.get("checkin"),
            None,
            "nothing saved"
        );

        table
            .set("checkin", checkin("簽到"), |s| s.checkin.card_slots = 7)
            .unwrap();
        let saved = settings::load(&dir);
        assert_eq!(saved.chat_commands["checkin"], checkin("簽到"));
        assert_eq!(saved.checkin.card_slots, 7, "written together");
        // Its own name again is fine; another command cannot take it.
        table.set("checkin", checkin("簽到"), |_| {}).unwrap();
        assert!(matches!(
            table.set("counter.deaths", entry("簽到", "counter", None), |_| {}),
            Err(CommandError::Taken { ref module, .. }) if module == "checkin"
        ));

        // A command saved without a declaration (a counter's) is in the table too, named by its label.
        table
            .set(
                "counter.x",
                entry("死亡", "counter", Some("死亡次數")),
                |_| {},
            )
            .unwrap();
        match table.set("checkin", checkin("死亡"), |_| {}) {
            Err(CommandError::Taken { module, label }) => {
                assert_eq!((module.as_str(), label.as_str()), ("counter", "死亡次數"))
            }
            other => panic!("{other:?}"),
        }
        table.remove("counter.x").unwrap();
        table.set("checkin", checkin("死亡"), |_| {}).unwrap();
        assert!(!settings::load(&dir).chat_commands.contains_key("counter.x"));

        // A fresh load reads what was saved.
        assert_eq!(
            CommandTable::load(dir.clone(), DECLARED.to_vec()).get("checkin"),
            checkin("死亡")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_entry_saved_before_modules_were_kept_gets_its_module() {
        let dir = std::env::temp_dir().join(format!("st-commands-old-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"chat_commands":{"checkin":{"name":"簽到"}}}"#,
        )
        .unwrap();
        let table = CommandTable::load(dir.clone(), DECLARED.to_vec());
        assert_eq!(table.get("checkin").module, "checkin");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn commands_match_the_first_word_ignoring_case() {
        let e = CommandEntry {
            name: "Deaths".into(),
            allowed: Allowed::Everyone,
            module: "counter".into(),
            label: None,
        };
        assert!(e.matches("!deaths"));
        assert!(e.matches("!DEATHS again"));
        assert!(!e.matches("deaths"));
        assert!(!e.matches("hi !deaths"));
        assert!(!e.matches("!death"));
    }
}
