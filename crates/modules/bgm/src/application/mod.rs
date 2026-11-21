//! The music library: imported files are copied into the data folder's `music/` under a UUID v7 name; their titles,
//! lengths and the playlists live in `settings.json`.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use toolkit_core::settings::{self, BgmSettings, MusicEntry, Playlist, SceneRule};
use uuid::Uuid;

use crate::domain::{self, Format};

pub mod player;

#[derive(Debug, thiserror::Error)]
pub enum BgmError {
    #[error("music.unsupported_type")]
    UnsupportedType,
    #[error("music.too_large")]
    TooLarge,
    #[error("music.not_found")]
    NotFound,
    #[error("music.invalid")]
    Invalid,
    /// No playlist chosen, or none of its tracks can play (missing files, lengths not measured yet).
    #[error("music.nothing_to_play")]
    NothingToPlay,
    /// The background music feature is turned off.
    #[error("music.off")]
    Off,
    #[error("music.save_failed")]
    SaveFailed(#[from] io::Error),
}

pub struct BgmService {
    data_dir: PathBuf,
}

impl BgmService {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    pub(crate) fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    fn folder(&self) -> PathBuf {
        self.data_dir.join("music")
    }

    /// The tracks in import order, and the playlists.
    pub fn library(&self) -> BgmSettings {
        settings::load(&self.data_dir).bgm
    }

    /// Copies `bytes` in as a new track; `file_name` only gives the format and the starting title.
    pub fn import(&self, file_name: &str, bytes: &[u8]) -> Result<MusicEntry, BgmError> {
        if bytes.len() > domain::MAX_BYTES {
            return Err(BgmError::TooLarge);
        }
        let format = Format::detect(file_name, bytes).ok_or(BgmError::UnsupportedType)?;
        let entry = MusicEntry {
            id: Uuid::now_v7().to_string(),
            title: toolkit_core::audio::label_from_file_name(file_name, "music"),
            format: format.extension().into(),
            duration_ms: None,
        };
        fs::create_dir_all(self.folder())?;
        let path = file_path(&self.folder(), &entry.id, format);
        fs::write(&path, bytes)?;
        if let Err(e) = settings::update(&self.data_dir, |s| s.bgm.library.push(entry.clone())) {
            let _ = fs::remove_file(&path);
            return Err(e.into());
        }
        Ok(entry)
    }

    pub fn rename(&self, id: &str, title: &str) -> Result<MusicEntry, BgmError> {
        let title = domain::clean_label(title).ok_or(BgmError::Invalid)?;
        let mut found = None;
        settings::update(&self.data_dir, |s| {
            if let Some(e) = s.bgm.library.iter_mut().find(|e| e.id == id) {
                e.title = title;
                found = Some(e.clone());
            }
        })?;
        found.ok_or(BgmError::NotFound)
    }

    /// Records how long a track plays, as the admin window measured it (1 ms to 24 hours).
    pub fn set_duration(&self, id: &str, duration_ms: u32) -> Result<(), BgmError> {
        if !(1..=domain::DURATION_MAX_MS).contains(&duration_ms) {
            return Err(BgmError::Invalid);
        }
        let mut found = false;
        settings::update(&self.data_dir, |s| {
            if let Some(e) = s.bgm.library.iter_mut().find(|e| e.id == id) {
                e.duration_ms = Some(duration_ms);
                found = true;
            }
        })?;
        found.then_some(()).ok_or(BgmError::NotFound)
    }

    /// Removes the track from the library and every playlist, then its file.
    pub fn delete(&self, id: &str) -> Result<(), BgmError> {
        let mut removed = None;
        settings::update(&self.data_dir, |s| {
            if let Some(i) = s.bgm.library.iter().position(|e| e.id == id) {
                removed = Some(s.bgm.library.remove(i));
                for p in &mut s.bgm.playlists {
                    p.tracks.retain(|t| t != id);
                }
            }
        })?;
        let entry = removed.ok_or(BgmError::NotFound)?;
        if let Some((path, _)) = self.file_of(&entry)
            && let Err(e) = fs::remove_file(&path)
            && e.kind() != io::ErrorKind::NotFound
        {
            // The library no longer lists it, so nothing plays it; the file only takes space.
            tracing::warn!(code = "music.file_not_removed", error = %e, "left a music file behind");
        }
        Ok(())
    }

    /// Puts the track in exactly `playlists`: added at the end of the ones it was not in, removed from the rest.
    pub fn set_playlists(&self, id: &str, playlists: &[String]) -> Result<(), BgmError> {
        let mut known = false;
        settings::update(&self.data_dir, |s| {
            known = s.bgm.library.iter().any(|e| e.id == id)
                && playlists
                    .iter()
                    .all(|p| s.bgm.playlists.iter().any(|q| q.id == *p));
            if !known {
                return;
            }
            for p in &mut s.bgm.playlists {
                let wanted = playlists.contains(&p.id);
                let has = p.tracks.iter().any(|t| t == id);
                if wanted && !has {
                    p.tracks.push(id.into());
                } else if !wanted && has {
                    p.tracks.retain(|t| t != id);
                }
            }
        })?;
        known.then_some(()).ok_or(BgmError::NotFound)
    }

    pub fn create_playlist(&self, name: &str) -> Result<Playlist, BgmError> {
        let playlist = Playlist {
            id: Uuid::now_v7().to_string(),
            name: domain::clean_label(name).ok_or(BgmError::Invalid)?,
            tracks: Vec::new(),
        };
        settings::update(&self.data_dir, |s| s.bgm.playlists.push(playlist.clone()))?;
        Ok(playlist)
    }

    pub fn rename_playlist(&self, id: &str, name: &str) -> Result<Playlist, BgmError> {
        let name = domain::clean_label(name).ok_or(BgmError::Invalid)?;
        let mut found = None;
        settings::update(&self.data_dir, |s| {
            if let Some(p) = s.bgm.playlists.iter_mut().find(|p| p.id == id) {
                p.name = name;
                found = Some(p.clone());
            }
        })?;
        found.ok_or(BgmError::NotFound)
    }

    /// Removes the playlist only; its tracks stay in the library. The scene rules that played it lose it and are
    /// disabled, never moved to another playlist.
    pub fn delete_playlist(&self, id: &str) -> Result<(), BgmError> {
        let mut found = false;
        settings::update(&self.data_dir, |s| {
            let before = s.bgm.playlists.len();
            s.bgm.playlists.retain(|p| p.id != id);
            found = s.bgm.playlists.len() != before;
            for r in s
                .bgm
                .scene_rules
                .iter_mut()
                .filter(|r| r.playlist.as_deref() == Some(id))
            {
                r.playlist = None;
                r.enabled = false;
            }
        })?;
        found.then_some(()).ok_or(BgmError::NotFound)
    }

    /// Replaces every scene rule; a rule without a valid ID gets a new one. Scene names are trimmed, and blank or
    /// repeated ones (by any case) dropped.
    pub fn set_scene_rules(&self, rules: Vec<SceneRule>) -> Result<Vec<SceneRule>, BgmError> {
        if rules.len() > domain::SCENE_RULES_MAX {
            return Err(BgmError::Invalid);
        }
        let rules = rules
            .into_iter()
            .map(|mut r| {
                r.name = domain::clean_label(&r.name).ok_or(BgmError::Invalid)?;
                let mut scenes: Vec<String> = Vec::new();
                for scene in r.scenes.iter().map(|s| s.trim()) {
                    if !scene.is_empty()
                        && !scenes
                            .iter()
                            .any(|t| domain::same_scene(Some(t), Some(scene)))
                    {
                        scenes.push(scene.to_owned());
                    }
                }
                r.scenes = scenes;
                if r.volume > 100
                    || r.scenes
                        .iter()
                        .any(|s| s.chars().count() > domain::SCENE_NAME_MAX)
                    || (r.enabled && r.playlist.is_none())
                {
                    return Err(BgmError::Invalid);
                }
                if Uuid::try_parse(&r.id).is_err() {
                    r.id = Uuid::now_v7().to_string();
                }
                Ok(r)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut known = false;
        settings::update(&self.data_dir, |s| {
            known = rules
                .iter()
                .filter_map(|r| r.playlist.as_ref())
                .all(|p| s.bgm.playlists.iter().any(|q| q.id == *p));
            if known {
                s.bgm.scene_rules = rules.clone();
            }
        })?;
        known.then_some(rules).ok_or(BgmError::NotFound)
    }

    /// The library lists it but its file is gone from the music folder (deleted outside the app).
    pub fn file_missing(&self, entry: &MusicEntry) -> bool {
        !self.file_of(entry).is_some_and(|(path, _)| path.is_file())
    }

    /// Where a track's file is, and its format.
    pub(crate) fn file(&self, id: &str) -> Option<(PathBuf, Format)> {
        self.library()
            .library
            .iter()
            .find(|e| e.id == id)
            .and_then(|e| self.file_of(e))
    }

    /// The path is built from our own UUID and format only, so even an edited `settings.json` cannot point
    /// outside `music/`.
    fn file_of(&self, entry: &MusicEntry) -> Option<(PathBuf, Format)> {
        let id = Uuid::try_parse(&entry.id).ok()?;
        let format = Format::from_extension(&entry.format)?;
        Some((file_path(&self.folder(), &id.to_string(), format), format))
    }
}

fn file_path(folder: &Path, id: &str, format: Format) -> PathBuf {
    folder.join(format!("{id}.{}", format.extension()))
}
