//! User settings stored as `settings.json` in the data folder. Secrets never go here; the Twitch Client ID is
//! public by design.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub port: u16,
    pub twitch_client_id: String,
    /// Modules the broadcaster turned off; everything else is enabled.
    pub disabled_modules: Vec<String>,
    /// Disabled modules that skip outbox events instead of keeping them (paused, the default, keeps them).
    pub closed_modules: Vec<String>,
    /// Where obs-websocket listens; its password lives in the Windows Credential Manager.
    pub obs: ObsSettings,
    /// "Don't show again" on the close hint: ✕ hides the admin window without asking.
    pub skip_close_hint: bool,
    /// Admin home tile IDs in display order. `None`: never arranged, the admin UI shows its defaults (and nothing
    /// is written); `Some([])`: the broadcaster removed every tile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home_tiles: Option<Vec<String>>,
    /// The chat overlay template: a built-in ID, or `custom` for the data folder's `templates/chat/`. The Host
    /// checks it; an unknown value means the default.
    pub chat_template: String,
    /// The alert overlay's built-in template. The Host checks it; an unknown value means the default.
    pub alert_template: String,
    /// Check-in rules; the check-in module validates them and falls back to defaults for values it cannot use.
    pub checkin: CheckinSettings,
    /// Imported sound files; the sound module checks them.
    pub sound: SoundSettings,
    /// Look for a newer release on GitHub at start and every 24 h; off means no request at all.
    pub update_check: bool,
    /// The release the broadcaster chose to skip ("not this one"); only a newer one shows the notice again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dismissed_version: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundSettings {
    pub library: Vec<SoundEntry>,
    /// What plays where, e.g. `alert.follow` → a library sound's ID or `silent`. A missing slot plays its
    /// default: the built-in sound for alerts, nothing for check-ins.
    pub slots: BTreeMap<String, String>,
}

/// One imported sound; its file is `sounds/<id>.<format>` in the data folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundEntry {
    /// UUID v7.
    pub id: String,
    /// Shown to the broadcaster; starts as the original file name, which is never used as a path.
    pub label: String,
    /// 0–100 (%).
    pub volume: u8,
    /// `mp3`, `wav` or `ogg`.
    pub format: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CheckinSettings {
    /// The chat command without its `!`.
    pub command: String,
    /// IANA time zone the business day is judged in.
    pub time_zone: String,
    /// Local hour (0–23) a new business day starts.
    pub reset_hour: u32,
    /// Slots on a stamp card.
    pub card_slots: u32,
    /// Twitch ID of the channel points reward that checks in; `None`: chat only.
    pub reward_id: Option<String>,
}

impl Default for CheckinSettings {
    fn default() -> Self {
        Self {
            command: "打卡".into(),
            time_zone: "Asia/Taipei".into(),
            reset_hour: 0,
            card_slots: 10,
            reward_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ObsSettings {
    pub host: String,
    pub port: u16,
}

impl Default for ObsSettings {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 4455,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            port: crate::DEFAULT_PORT,
            twitch_client_id: String::new(),
            disabled_modules: Vec::new(),
            closed_modules: Vec::new(),
            obs: ObsSettings::default(),
            skip_close_hint: false,
            home_tiles: None,
            chat_template: "default".into(),
            alert_template: "default".into(),
            checkin: CheckinSettings::default(),
            sound: SoundSettings::default(),
            update_check: true,
            dismissed_version: None,
        }
    }
}

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join("settings.json")
}

/// Missing settings mean defaults. A file that cannot be read or parsed also falls back to defaults so startup is
/// never blocked, but is first copied to `settings.json.bak`: the next save would otherwise overwrite it.
pub fn load(config_dir: &Path) -> Settings {
    let path = path(config_dir);
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| {
            back_up(&path);
            Settings::default()
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Settings::default(),
        Err(_) => {
            back_up(&path);
            Settings::default()
        }
    }
}

/// Returns whether a copy was made. The admin UI polls settings, so an unchanged broken file is copied and
/// logged once, not on every load.
fn back_up(path: &Path) -> bool {
    let backup = path.with_extension("json.bak");
    if let (Ok(current), Ok(kept)) = (fs::read(path), fs::read(&backup))
        && current == kept
    {
        return false;
    }
    match fs::copy(path, &backup) {
        Ok(_) => {
            tracing::warn!(code = "settings.unreadable", backup = %backup.display(), "kept a copy");
            true
        }
        Err(e) => {
            tracing::error!(code = "settings.unreadable", error = %e, "could not keep a copy");
            false
        }
    }
}

pub fn save(config_dir: &Path, settings: &Settings) -> io::Result<()> {
    fs::create_dir_all(config_dir)?;
    let path = path(config_dir);
    // Write to a temp file then rename, so a crash never leaves a half-written settings file.
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(settings)?)?;
    fs::rename(tmp, path)
}

/// Loads, applies `change`, and saves in one step. Serialized so two callers never lose each other's change.
pub fn update(config_dir: &Path, change: impl FnOnce(&mut Settings)) -> io::Result<Settings> {
    static WRITER: Mutex<()> = Mutex::new(());
    let _writer = WRITER.lock().unwrap();
    let mut settings = load(config_dir);
    change(&mut settings);
    save(config_dir, &settings)?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_json_keeps_defaults() {
        let s: Settings = serde_json::from_str(r#"{"port":5000}"#).unwrap();
        assert_eq!(s.port, 5000);
        assert_eq!(s.twitch_client_id, "");
        assert_eq!(s.obs, ObsSettings::default());
        assert_eq!((s.obs.host.as_str(), s.obs.port), ("127.0.0.1", 4455));
    }

    #[test]
    fn broken_file_is_backed_up_before_defaults_are_used() {
        let dir = std::env::temp_dir().join(format!("st-settings-broken-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let broken = r#"{"port":"not a number","twitch_client_id":"abc"}"#;
        fs::write(path(&dir), broken).unwrap();
        assert_eq!(load(&dir), Settings::default());
        let backup = path(&dir).with_extension("json.bak");
        assert_eq!(fs::read_to_string(backup).unwrap(), broken);
        assert!(
            !back_up(&path(&dir)),
            "same broken file is not copied again"
        );
        fs::write(path(&dir), "{").unwrap();
        assert!(back_up(&path(&dir)), "a different broken file is copied");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn home_tiles_tell_never_arranged_from_emptied() {
        let never = serde_json::to_string(&Settings::default()).unwrap();
        assert!(!never.contains("home_tiles"), "None writes nothing");
        let emptied = Settings {
            home_tiles: Some(Vec::new()),
            ..Settings::default()
        };
        let text = serde_json::to_string(&emptied).unwrap();
        assert!(text.contains(r#""home_tiles":[]"#), "{text}");
        assert_eq!(
            serde_json::from_str::<Settings>(&text).unwrap().home_tiles,
            Some(Vec::new())
        );
        assert_eq!(
            serde_json::from_str::<Settings>(&never).unwrap().home_tiles,
            None
        );
    }

    #[test]
    fn update_round_trips() {
        let dir = std::env::temp_dir().join(format!("st-settings-{}", std::process::id()));
        let saved = update(&dir, |s| s.twitch_client_id = "abc".into()).unwrap();
        assert_eq!(load(&dir), saved);
        let _ = fs::remove_dir_all(dir);
    }
}
