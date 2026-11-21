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

/// One command as saved: the name viewers type without its `!`, and who may use it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandEntry {
    pub name: String,
    #[serde(default)]
    pub allowed: Allowed,
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

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    /// Another feature's command already has the name (ignoring case).
    #[error("commands.taken")]
    Taken { module: &'static str, name: String },
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
    /// The table in `data_dir`, for the commands every module declared.
    pub fn load(data_dir: PathBuf, declared: Vec<Declared>) -> Self {
        let saved = settings::load(&data_dir).chat_commands;
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

    /// The command for `key`: as saved, or its declared default for everyone.
    pub fn get(&self, key: &str) -> CommandEntry {
        self.saved(key).unwrap_or_else(|| CommandEntry {
            name: self
                .declared
                .iter()
                .find(|d| d.key == key)
                .map_or("", |d| d.default_name)
                .to_owned(),
            allowed: Allowed::Everyone,
        })
    }

    /// Saves `entry` for `key`, together with `also` in the same write of `settings.json`, unless another declared
    /// command has the same name (ignoring case). Nothing is saved when it fails.
    pub fn set(
        &self,
        key: &str,
        entry: CommandEntry,
        also: impl FnOnce(&mut Settings),
    ) -> Result<(), CommandError> {
        let mut saved = self.write();
        let name = entry.name.to_lowercase();
        for d in self.declared.iter().filter(|d| d.key != key) {
            let other = saved.get(d.key).map_or(d.default_name, |e| e.name.as_str());
            if other.to_lowercase() == name {
                return Err(CommandError::Taken {
                    module: d.module,
                    name: other.to_owned(),
                });
            }
        }
        settings::update(&self.data_dir, |s| {
            s.chat_commands.insert(key.to_owned(), entry.clone());
            also(s);
        })?;
        saved.insert(key.to_owned(), entry);
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
        assert_eq!(table.saved("checkin"), None);

        let entry = |name: &str| CommandEntry {
            name: name.into(),
            allowed: Allowed::Subscriber,
        };
        match table.set("checkin", entry("deaths"), |_| {}) {
            Err(CommandError::Taken { module, name }) => {
                assert_eq!((module, name.as_str()), ("counter", "Deaths"))
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            settings::load(&dir).chat_commands.get("checkin"),
            None,
            "nothing saved"
        );

        table
            .set("checkin", entry("簽到"), |s| s.checkin.card_slots = 7)
            .unwrap();
        let saved = settings::load(&dir);
        assert_eq!(saved.chat_commands["checkin"], entry("簽到"));
        assert_eq!(saved.checkin.card_slots, 7, "written together");
        // Its own name again is fine; another command cannot take it.
        table.set("checkin", entry("簽到"), |_| {}).unwrap();
        assert!(matches!(
            table.set("counter.deaths", entry("簽到"), |_| {}),
            Err(CommandError::Taken {
                module: "checkin",
                ..
            })
        ));
        // A fresh load reads what was saved.
        assert_eq!(
            CommandTable::load(dir.clone(), DECLARED.to_vec()).get("checkin"),
            entry("簽到")
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
