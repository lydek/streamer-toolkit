//! Module registry with the enable/disable switch. Disabling keeps a module's data; while disabled its
//! in-memory events (chat and the like) are dropped, not kept for later.

use std::{collections::HashSet, io, path::PathBuf, sync::RwLock};

use serde::Serialize;

use crate::settings;

/// Modules that exist in this build. A module is enabled unless the broadcaster turned it off.
pub const CHAT: &str = "chat";
pub const ALL: &[&str] = &[CHAT];

#[derive(Debug, thiserror::Error)]
pub enum ModuleError {
    #[error("modules.unknown")]
    Unknown,
    #[error("modules.save_failed")]
    SaveFailed(#[from] io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModuleState {
    pub id: &'static str,
    pub enabled: bool,
}

pub struct Modules {
    data_dir: PathBuf,
    disabled: RwLock<HashSet<String>>,
}

impl Modules {
    pub fn load(data_dir: PathBuf) -> Self {
        let disabled = settings::load(&data_dir)
            .disabled_modules
            .into_iter()
            .collect();
        Self {
            data_dir,
            disabled: RwLock::new(disabled),
        }
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        !self.disabled.read().unwrap().contains(id)
    }

    pub fn list(&self) -> Vec<ModuleState> {
        ALL.iter()
            .map(|id| ModuleState {
                id,
                enabled: self.is_enabled(id),
            })
            .collect()
    }

    /// Saves first, so the switch never claims a state that is not on disk.
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), ModuleError> {
        if !ALL.contains(&id) {
            return Err(ModuleError::Unknown);
        }
        let mut disabled = self.disabled.write().unwrap();
        let mut next = disabled.clone();
        if enabled {
            next.remove(id);
        } else {
            next.insert(id.to_owned());
        }
        let mut list: Vec<String> = next.iter().cloned().collect();
        list.sort();
        settings::update(&self.data_dir, |s| s.disabled_modules = list)?;
        *disabled = next;
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
        let _ = std::fs::remove_dir_all(dir);
    }
}
