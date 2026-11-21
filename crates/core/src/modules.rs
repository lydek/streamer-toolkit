//! Module registry with the enable/disable switch. Disabling keeps a module's data; while disabled its
//! in-memory events (chat and the like) are dropped, not kept for later.

use std::{collections::HashSet, io, path::PathBuf, sync::RwLock};

use serde::Serialize;

use crate::settings;

/// Modules that exist in this build. A module is enabled unless the broadcaster turned it off.
pub const CHAT: &str = "chat";
pub const ALERTS: &str = "alerts";
pub const REDEMPTION: &str = "redemption";
pub const CHECKIN: &str = "checkin";
pub const SOUND: &str = "sound";
/// OBS scenes and items, through the streaming-software port; the switch only shows or hides its pages.
pub const SCENE: &str = "scene";
/// Background music: the music library and its playlists.
pub const BGM: &str = "bgm";
pub const ALL: &[&str] = &[CHAT, ALERTS, REDEMPTION, CHECKIN, SOUND, SCENE, BGM];

#[derive(Debug, thiserror::Error)]
pub enum ModuleError {
    #[error("modules.unknown")]
    Unknown,
    #[error("modules.save_failed")]
    SaveFailed(#[from] io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct ModuleState {
    pub id: &'static str,
    pub enabled: bool,
    /// Disabled and skipping its outbox events ("close"); a disabled module that is not closed is paused and keeps
    /// them for later.
    pub closed: bool,
}

#[derive(Default)]
struct Switches {
    disabled: HashSet<String>,
    closed: HashSet<String>,
}

pub struct Modules {
    data_dir: PathBuf,
    switches: RwLock<Switches>,
}

fn sorted(set: &HashSet<String>) -> Vec<String> {
    let mut list: Vec<String> = set.iter().cloned().collect();
    list.sort();
    list
}

impl Modules {
    pub fn load(data_dir: PathBuf) -> Self {
        let s = settings::load(&data_dir);
        let disabled: HashSet<String> = s.disabled_modules.into_iter().collect();
        let closed = s
            .closed_modules
            .into_iter()
            .filter(|m| disabled.contains(m))
            .collect();
        Self {
            data_dir,
            switches: RwLock::new(Switches { disabled, closed }),
        }
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        !self.switches.read().unwrap().disabled.contains(id)
    }

    /// Disabled with "close": its outbox events are skipped, never delivered later.
    pub fn is_closed(&self, id: &str) -> bool {
        self.switches.read().unwrap().closed.contains(id)
    }

    pub fn list(&self) -> Vec<ModuleState> {
        ALL.iter()
            .map(|id| ModuleState {
                id,
                enabled: self.is_enabled(id),
                closed: self.is_closed(id),
            })
            .collect()
    }

    /// Enables, or disables as paused (outbox events kept for later).
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), ModuleError> {
        self.set(id, !enabled, false)
    }

    /// Disables as closed (outbox events skipped); a paused module switches to closed.
    pub fn close(&self, id: &str) -> Result<(), ModuleError> {
        self.set(id, true, true)
    }

    /// Saves first, so the switch never claims a state that is not on disk.
    fn set(&self, id: &str, disabled: bool, closed: bool) -> Result<(), ModuleError> {
        if !ALL.contains(&id) {
            return Err(ModuleError::Unknown);
        }
        let mut switches = self.switches.write().unwrap();
        let mut next_disabled = switches.disabled.clone();
        let mut next_closed = switches.closed.clone();
        if disabled {
            next_disabled.insert(id.to_owned());
        } else {
            next_disabled.remove(id);
        }
        if closed {
            next_closed.insert(id.to_owned());
        } else {
            next_closed.remove(id);
        }
        settings::update(&self.data_dir, |s| {
            s.disabled_modules = sorted(&next_disabled);
            s.closed_modules = sorted(&next_closed);
        })?;
        *switches = Switches {
            disabled: next_disabled,
            closed: next_closed,
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_persists_and_rejects_unknown_modules() {
        let dir = std::env::temp_dir().join(format!("st-modules-{}", std::process::id()));
        let modules = Modules::load(dir.clone());
        assert!(modules.is_enabled(CHAT));

        modules.set_enabled(CHAT, false).unwrap();
        assert!(!modules.is_enabled(CHAT));
        assert!(
            !Modules::load(dir.clone()).is_enabled(CHAT),
            "kept across restarts"
        );

        modules.set_enabled(CHAT, true).unwrap();
        assert!(Modules::load(dir.clone()).is_enabled(CHAT));
        assert!(matches!(
            modules.set_enabled("nope", false),
            Err(ModuleError::Unknown)
        ));

        modules.close(CHECKIN).unwrap();
        let reloaded = Modules::load(dir.clone());
        assert!(
            !reloaded.is_enabled(CHECKIN) && reloaded.is_closed(CHECKIN),
            "closed is kept"
        );
        modules.set_enabled(CHECKIN, false).unwrap();
        assert!(!modules.is_closed(CHECKIN), "disabling again pauses");
        modules.close(CHECKIN).unwrap();
        modules.set_enabled(CHECKIN, true).unwrap();
        assert!(modules.is_enabled(CHECKIN) && !modules.is_closed(CHECKIN));
        let _ = std::fs::remove_dir_all(dir);
    }
}
