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
    /// The warnings, media and marathon switches were split out of screen tools and counters (once, on the first
    /// start that has them).
    pub modules_split: bool,
    /// Where obs-websocket listens; its password lives in the Windows Credential Manager.
    pub obs: ObsSettings,
    /// "Don't show again" on the close hint: ✕ hides the admin window without asking.
    pub skip_close_hint: bool,
    /// Admin home tile IDs in display order. `None`: never arranged, the admin UI shows its defaults (and nothing
    /// is written); `Some([])`: the broadcaster removed every tile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home_tiles: Option<Vec<String>>,
    /// The 開播前 and 下播後 homes, like `home_tiles`, which is the 直播中 one (it was the only home before).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home_tiles_before: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home_tiles_after: Option<Vec<String>>,
    /// Which home `home_tiles` was saved for (0: tiles in two groups, 1: one grid, 2: one grid with the stage
    /// blocks, 3: the 直播中 home of three). An older list is brought up to date once, the first time it is read (`crates/host/src/home_tiles.rs`).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub home_tiles_version: u8,
    /// The admin UI's docked chat panel. `None`: never changed, the admin UI shows its defaults (closed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_dock: Option<ChatDock>,
    /// How the admin UI shows chat. `None`: never changed, the admin UI's defaults.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_display: Option<ChatDisplay>,
    /// How the 下播後 home shows its numbers. `None`: never changed, they show.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_numbers: Option<AfterNumbers>,
    /// The admin UI's tab bar. `None`: never edited, every group is in the bar on top.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_bar: Option<TabBar>,
    /// The admin UI's text size in percent (the Host checks the range). `None`: never changed, 100.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_size: Option<u16>,
    /// How long chat and activity records are kept. `None`: never changed, [`RecordRetention::DEFAULT`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_retention: Option<RecordRetention>,
    /// The chat overlay template: a built-in ID, or `custom` for the data folder's `templates/chat/`. The Host
    /// checks it; an unknown value means the default.
    pub chat_template: String,
    /// The chat overlay's text size in px (16–48); `None` keeps each template's own size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_text_size: Option<u16>,
    /// The alert overlay's built-in template. The Host checks it; an unknown value means the default.
    pub alert_template: String,
    /// Check-in rules; the check-in module validates them and falls back to defaults for values it cannot use.
    pub checkin: CheckinSettings,
    /// Every feature's chat commands by key (`commands::Declared`); a key not here has its declared default.
    pub chat_commands: BTreeMap<String, crate::commands::CommandEntry>,
    /// Imported sound files; the sound module checks them.
    pub sound: SoundSettings,
    /// Imported background music and its playlists; the bgm module checks them.
    pub bgm: BgmSettings,
    /// Look for a newer release on GitHub at start and every 24 h; off means no request at all.
    pub update_check: bool,
    /// The release the broadcaster chose to skip ("not this one"); only a newer one shows the notice again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dismissed_version: Option<String>,
}

/// Whether the admin UI docks the chat on wide windows, how wide (CSS pixels; the Host checks the range) and on
/// which side of the pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ChatDock {
    pub open: bool,
    pub width: u16,
    /// Missing in settings saved before the choice existed: the right side.
    #[serde(default)]
    pub side: DockSide,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum DockSide {
    Left,
    #[default]
    Right,
}

/// How the admin UI's 下播後 home shows its numbers, so a streamer can look back without number anxiety. `revealed`:
/// numbers shown one by one while masked (the admin UI's IDs, kept until shown all or hidden again).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AfterNumbers {
    pub mode: NumbersMode,
    pub revealed: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum NumbersMode {
    /// Every number shows.
    Show,
    /// Each number shows as 「––」 until revealed.
    Mask,
    /// The tiles are folded away behind one card.
    Hide,
}

/// How the admin UI shows chat (the chat log page and the docked panel); OBS overlays follow their template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatDisplay {
    pub style: ChatStyle,
    pub avatars: bool,
    pub badges: bool,
    pub time: bool,
    /// Percent of the admin UI's text size, 80-150 in steps of 10 (the Host checks).
    pub text_size: u16,
}

/// How much room each chat line takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChatStyle {
    Compact,
    Standard,
    Comfortable,
}

/// Which tab groups the admin UI pins in its tab bar, in order (the rest go into "More"; `[]` pins none), and
/// where the bar sits. The Host checks the format only; which group IDs exist is the admin UI's business.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct TabBar {
    pub groups: Vec<String>,
    pub position: TabBarPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum TabBarPosition {
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundSettings {
    pub library: Vec<SoundEntry>,
    /// What plays where, e.g. `alert.follow` → a library sound's ID or `silent`. A missing slot plays its
    /// default: the built-in sound for alerts, nothing for check-ins.
    pub slots: BTreeMap<String, String>,
    /// Master volume, 0–100 (%), applied on top of each sound's own: reward, alert and check-in sounds.
    pub volume: u8,
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self {
            library: Vec::new(),
            slots: BTreeMap::new(),
            volume: 100,
        }
    }
}

/// Keep records for this many days (1–3650), or forever; older ones are deleted on their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecordRetention {
    Days(u16),
    Forever,
}

impl RecordRetention {
    pub const DEFAULT: Self = Self::Days(30);
    pub const MAX_DAYS: u16 = 3650;

    pub fn valid(self) -> bool {
        match self {
            Self::Days(days) => (1..=Self::MAX_DAYS).contains(&days),
            Self::Forever => true,
        }
    }
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
    /// How long it plays, measured by the admin window; `None` until measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BgmSettings {
    pub library: Vec<MusicEntry>,
    pub playlists: Vec<Playlist>,
    /// The player's volume, 0–100 (%).
    pub volume: u8,
    /// The player plays playlists in random order instead of their own.
    pub shuffle: bool,
    /// What plays on which OBS scene; with none, switching scenes leaves the player alone.
    pub scene_rules: Vec<SceneRule>,
}

impl Default for BgmSettings {
    fn default() -> Self {
        Self {
            library: Vec::new(),
            playlists: Vec::new(),
            volume: 50,
            shuffle: false,
            scene_rules: Vec::new(),
        }
    }
}

/// A playlist that plays on some OBS scenes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneRule {
    /// UUID v7.
    pub id: String,
    pub name: String,
    /// Scene names, compared without regard to case.
    pub scenes: Vec<String>,
    /// `None` once its playlist was deleted; the rule is then disabled.
    pub playlist: Option<String>,
    pub shuffle: bool,
    /// 0–100 (%).
    pub volume: u8,
    /// Switching to one of its scenes starts its playlist.
    pub autoplay: bool,
    /// Switching away to a scene whose rule does not autoplay stops the music.
    pub stop_on_leave: bool,
    pub enabled: bool,
}

/// One imported track; its file is `music/<id>.<format>` in the data folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MusicEntry {
    /// UUID v7.
    pub id: String,
    /// Shown to the broadcaster; starts as the original file name, which is never used as a path.
    pub title: String,
    /// `mp3`, `wav` or `ogg`.
    pub format: String,
    /// How long it plays, measured by the admin window; `None` until measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playlist {
    /// UUID v7.
    pub id: String,
    pub name: String,
    /// Track IDs in play order; a track can be in several playlists.
    pub tracks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CheckinSettings {
    /// The chat command without its `!`. Only read: it moved to `chat_commands` (key `checkin`), and the
    /// check-in module carries an older file's value over once.
    #[serde(skip_serializing)]
    pub command: String,
    /// IANA time zone the business day is judged in.
    pub time_zone: String,
    /// Local hour (0–23) a new business day starts.
    pub reset_hour: u32,
    /// Slots on a stamp card.
    pub card_slots: u32,
    /// Twitch ID of the channel points reward that checks in; `None`: chat only.
    pub reward_id: Option<String>,
    /// How the stamp card looks on the overlay; the check-in module checks it.
    pub card: CardStyle,
}

impl Default for CheckinSettings {
    fn default() -> Self {
        Self {
            command: "打卡".into(),
            time_zone: "Asia/Taipei".into(),
            reset_hour: 0,
            card_slots: 10,
            reward_id: None,
            card: CardStyle::default(),
        }
    }
}

/// The stamp card's look. Images are file names in the data folder's `checkin/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CardStyle {
    /// `simple` or `journal`.
    pub preset: String,
    /// `#RRGGBB`.
    pub background_color: String,
    pub background_image: Option<String>,
    /// `#RRGGBB`.
    pub stamp_color: String,
    pub stamp_image: Option<String>,
    /// Slots per row.
    pub per_row: u32,
    /// May hold `#觀眾#`, `#章數#`, `#次數#` and `#格數#`.
    pub title: String,
    /// May hold the same variables as `title`.
    pub description: String,
    /// `fade`, `slide`, `pop` or `none`.
    pub animation: String,
    /// A repeat check-in on the same business day shows the card too (nothing is recorded).
    pub show_repeat: bool,
}

impl Default for CardStyle {
    fn default() -> Self {
        Self {
            preset: "simple".into(),
            background_color: "#181624".into(),
            background_image: None,
            stamp_color: "#d8b7ff".into(),
            stamp_image: None,
            per_row: 10,
            title: "#觀眾#".into(),
            description: "累計 #次數# 次".into(),
            animation: "slide".into(),
            show_repeat: false,
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
            modules_split: false,
            obs: ObsSettings::default(),
            skip_close_hint: false,
            home_tiles: None,
            home_tiles_before: None,
            home_tiles_after: None,
            home_tiles_version: 0,
            chat_dock: None,
            chat_display: None,
            after_numbers: None,
            tab_bar: None,
            text_size: None,
            record_retention: None,
            chat_template: "default".into(),
            chat_text_size: None,
            alert_template: "default".into(),
            checkin: CheckinSettings::default(),
            chat_commands: BTreeMap::new(),
            sound: SoundSettings::default(),
            bgm: BgmSettings::default(),
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

fn is_zero(n: &u8) -> bool {
    *n == 0
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
