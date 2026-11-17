//! User settings stored as `settings.json` in the data folder. Secrets never go here; the Twitch Client ID is
//! public by design.

use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
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
/// never blocked; [`update`] then refuses to write, so the file is never overwritten.
pub fn load(config_dir: &Path) -> Settings {
    read(config_dir).unwrap_or_default()
}

/// Like [`load`], but a file that exists and cannot be read or parsed is an error (`InvalidData` when it does
/// not parse). Each broken version is first copied next to it, see [`back_up`].
pub fn read(config_dir: &Path) -> io::Result<Settings> {
    let path = path(config_dir);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(e) => {
            back_up(&path);
            return Err(e);
        }
    };
    serde_json::from_str(&text).map_err(|e| {
        back_up(&path);
        io::Error::new(io::ErrorKind::InvalidData, e)
    })
}

/// Copies a broken settings file to `settings.json.<UTC time>.bak`; returns whether a copy was made. Each broken
/// version is copied once (the admin UI polls settings), and never over an earlier copy.
fn back_up(path: &Path) -> bool {
    let current = match fs::read(path) {
        Ok(current) => current,
        Err(e) => {
            tracing::error!(code = "settings.unreadable", error = %e, "could not keep a copy");
            return false;
        }
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    let kept = fs::read_dir(dir).into_iter().flatten().flatten().any(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        name.starts_with("settings.json.")
            && name.ends_with(".bak")
            && fs::read(e.path()).is_ok_and(|b| b == current)
    });
    if kept {
        return false;
    }
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%3fZ");
    let backup = path.with_extension(format!("json.{stamp}.bak"));
    match write_synced(&backup, &current) {
        Ok(()) => {
            tracing::warn!(code = "settings.unreadable", backup = %backup.display(), "kept a copy");
            true
        }
        Err(e) => {
            tracing::error!(code = "settings.unreadable", error = %e, "could not keep a copy");
            false
        }
    }
}

/// Writes and flushes to disk before returning.
fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

pub fn save(config_dir: &Path, settings: &Settings) -> io::Result<()> {
    fs::create_dir_all(config_dir)?;
    let path = path(config_dir);
    // Written to disk before the rename, so a crash or power cut never leaves a half-written or empty file.
    let tmp = path.with_extension("json.tmp");
    write_synced(&tmp, &serde_json::to_vec_pretty(settings)?)?;
    fs::rename(tmp, path)
}

/// Reads, applies `change`, and saves in one step. Serialized so two callers never lose each other's change. A
/// settings file that cannot be read or parsed is an error and stays as it is.
pub fn update(config_dir: &Path, change: impl FnOnce(&mut Settings)) -> io::Result<Settings> {
    static WRITER: Mutex<()> = Mutex::new(());
    let _writer = WRITER.lock().unwrap();
    let mut settings = read(config_dir)?;
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

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-settings-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn backups(dir: &Path) -> Vec<String> {
        let mut kept: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "bak"))
            .map(|p| fs::read_to_string(p).unwrap())
            .collect();
        kept.sort();
        kept
    }

    #[test]
    fn broken_file_is_backed_up_before_defaults_are_used() {
        let dir = temp_dir("broken");
        let broken = r#"{"port":"not a number","twitch_client_id":"abc"}"#;
        fs::write(path(&dir), broken).unwrap();
        assert_eq!(load(&dir), Settings::default());
        assert_eq!(backups(&dir), [broken]);
        assert!(
            !back_up(&path(&dir)),
            "same broken file is not copied again"
        );
        fs::write(path(&dir), "{").unwrap();
        assert!(back_up(&path(&dir)), "a different broken file is copied");
        assert_eq!(backups(&dir), ["{", broken], "never over an earlier copy");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_broken_file_is_never_overwritten_by_a_save() {
        let dir = temp_dir("never-overwritten");
        let broken = r#"{"port":5000,"twitch_client_id":"abc""#;
        fs::write(path(&dir), broken).unwrap();
        let e = update(&dir, |s| s.skip_close_hint = true).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        assert!(read(&dir).is_err());
        assert_eq!(
            fs::read_to_string(path(&dir)).unwrap(),
            broken,
            "byte for byte"
        );
        assert_eq!(backups(&dir), [broken]);
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
