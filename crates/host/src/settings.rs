//! User settings stored as JSON in the app config directory (kept apart from data, cache and credentials).
//! Secrets never go here; the Twitch Client ID is public by design.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub port: u16,
    pub twitch_client_id: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            port: crate::DEFAULT_PORT,
            twitch_client_id: String::new(),
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

fn back_up(path: &Path) {
    let backup = path.with_extension("json.bak");
    // shortcut: stderr until structured logging lands with the skeleton foundations.
    match fs::copy(path, &backup) {
        Ok(_) => eprintln!("settings unreadable, kept a copy at {}", backup.display()),
        Err(e) => eprintln!("settings unreadable and could not be copied: {e}"),
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

/// Loads, applies `change`, and saves in one step.
pub fn update(config_dir: &Path, change: impl FnOnce(&mut Settings)) -> io::Result<Settings> {
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
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn update_round_trips() {
        let dir = std::env::temp_dir().join(format!("st-settings-{}", std::process::id()));
        let saved = update(&dir, |s| s.twitch_client_id = "abc".into()).unwrap();
        assert_eq!(load(&dir), saved);
        let _ = fs::remove_dir_all(dir);
    }
}
