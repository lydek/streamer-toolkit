//! Module registry with the enable/disable switch. Disabling keeps a module's data; while disabled its
//! in-memory events (chat and the like) are dropped, not kept for later.

use std::{collections::HashSet, io, path::PathBuf, sync::RwLock};

use serde::Serialize;

use crate::settings;

/// Modules that exist in this build. A module is enabled unless the broadcaster turned it off (or it starts off,
/// `OFF_BY_DEFAULT`, and was not turned on yet).
pub const CHAT: &str = "chat";
pub const ALERTS: &str = "alerts";
pub const REDEMPTION: &str = "redemption";
pub const CHECKIN: &str = "checkin";
pub const SOUND: &str = "sound";
/// OBS scenes and items, through the streaming-software port; the switch only shows or hides its pages.
pub const SCENE: &str = "scene";
/// Background music: the music library and its playlists.
pub const BGM: &str = "bgm";
/// Counters: global counts with chat commands.
pub const COUNTER: &str = "counter";
/// The sub marathon timer: time added by subs and bits, on its own overlay.
pub const MARATHON: &str = "marathon";
/// The break card; the ID is its page's path, so its home tile keeps the ID it had while unbuilt (it was all of
/// the screen tools until the warnings and media got switches of their own).
pub const SCREEN_TOOLS: &str = "screen-tools";
/// The on-stream warnings.
pub const WARNINGS: &str = "warnings";
/// Videos and GIFs played on a redemption (on the alerts overlay).
pub const MEDIA: &str = "media";
/// 音訊混音: OBS's audio inputs, through the streaming-software port; the switch only shows or hides its page and
/// tile.
pub const MIXER: &str = "mixer";
/// 直播資訊: the title, category and tags on Twitch; the switch only shows or hides its page and tile.
pub const STREAM_INFO: &str = "stream-info";
/// 頻道點數獎勵管理: listing and changing the channel's custom rewards; the switch only shows or hides its page and
/// tile.
pub const REWARDS: &str = "rewards";
pub const ALL: &[&str] = &[
    CHAT,
    ALERTS,
    REDEMPTION,
    CHECKIN,
    SOUND,
    SCENE,
    BGM,
    COUNTER,
    MARATHON,
    SCREEN_TOOLS,
    WARNINGS,
    MEDIA,
    MIXER,
    STREAM_INFO,
    REWARDS,
];

/// Modules that start off: each needs a Twitch scope the base sign-in lacks, so turning it on asks for that first.
pub const OFF_BY_DEFAULT: &[&str] = &[STREAM_INFO, REWARDS];

/// Modules split out of another one, with the one they came from: each starts in that one's state (off, closed).
const SPLIT: &[(&str, &str)] = &[
    (WARNINGS, SCREEN_TOOLS),
    (MEDIA, SCREEN_TOOLS),
    (MARATHON, COUNTER),
];

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

/// Turns off each module that starts off and was never put off before, and notes it; `true` when anything changed.
fn default_off(s: &mut settings::Settings) -> bool {
    let mut changed = false;
    for id in OFF_BY_DEFAULT {
        if !s.modules_defaulted.iter().any(|m| m == id) {
            s.modules_defaulted.push((*id).to_owned());
            if !s.disabled_modules.iter().any(|m| m == id) {
                s.disabled_modules.push((*id).to_owned());
            }
            changed = true;
        }
    }
    s.disabled_modules.sort();
    changed
}

/// Gives each split-out module the state of the module it came from.
fn split(s: &mut settings::Settings) {
    for (new, old) in SPLIT {
        for list in [&mut s.disabled_modules, &mut s.closed_modules] {
            if list.iter().any(|m| m == old) && !list.iter().any(|m| m == new) {
                list.push((*new).to_owned());
            }
        }
    }
    s.disabled_modules.sort();
    s.closed_modules.sort();
}

fn sorted(set: &HashSet<String>) -> Vec<String> {
    let mut list: Vec<String> = set.iter().cloned().collect();
    list.sort();
    list
}

impl Modules {
    pub fn load(data_dir: PathBuf) -> Self {
        let mut s = settings::load(&data_dir);
        let split_now = !s.modules_split;
        if split_now {
            split(&mut s);
        }
        if default_off(&mut s) || split_now {
            // Saved so it happens once; a failed save tries again next start, which gives the same states.
            if let Err(e) = settings::update(&data_dir, |saved| {
                saved.disabled_modules = s.disabled_modules.clone();
                saved.closed_modules = s.closed_modules.clone();
                saved.modules_split = true;
                saved.modules_defaulted = s.modules_defaulted.clone();
            }) {
                tracing::warn!(error = %e, "split or default-off module switches not saved");
            }
        }
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
    #[test]
    fn split_out_modules_start_in_the_state_of_the_one_they_came_from_once() {
        let dir = std::env::temp_dir().join(format!("st-modules-split-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        settings::update(&dir, |s| {
            s.disabled_modules = vec!["counter".into(), "screen-tools".into()];
            s.closed_modules = vec!["screen-tools".into()];
        })
        .unwrap();
        let m = Modules::load(dir.clone());
        for id in [SCREEN_TOOLS, WARNINGS, MEDIA] {
            assert!(!m.is_enabled(id) && m.is_closed(id), "{id}");
        }
        assert!(!m.is_enabled(MARATHON) && !m.is_closed(MARATHON));
        // Turned back on afterwards, it stays on after a restart: the split happens once.
        m.set_enabled(WARNINGS, true).unwrap();
        assert!(Modules::load(dir.clone()).is_enabled(WARNINGS));
        let _ = std::fs::remove_dir_all(dir);
    }

    use super::*;

    #[test]
    fn modules_needing_a_scope_start_off_once() {
        let dir =
            std::env::temp_dir().join(format!("st-modules-default-off-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let m = Modules::load(dir.clone());
        assert!(!m.is_enabled(STREAM_INFO) && !m.is_closed(STREAM_INFO));
        assert!(m.is_enabled(CHAT));
        // Turned on, it stays on after a restart.
        m.set_enabled(STREAM_INFO, true).unwrap();
        assert!(Modules::load(dir.clone()).is_enabled(STREAM_INFO));
        let _ = std::fs::remove_dir_all(dir);
    }

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
