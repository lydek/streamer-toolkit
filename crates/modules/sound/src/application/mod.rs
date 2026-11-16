//! The sound library: imported files are copied into the data folder's `sounds/` under a UUID v7 name, and their
//! label and volume live in `settings.json`.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use toolkit_core::settings::{self, SoundEntry};
use uuid::Uuid;

use crate::domain::{self, Format};

#[derive(Debug, thiserror::Error)]
pub enum SoundError {
    #[error("sound.unsupported_type")]
    UnsupportedType,
    #[error("sound.too_large")]
    TooLarge,
    #[error("sound.not_found")]
    NotFound,
    #[error("sound.invalid")]
    Invalid,
    #[error("sound.save_failed")]
    SaveFailed(#[from] io::Error),
}

pub struct SoundService {
    data_dir: PathBuf,
}

impl SoundService {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    fn folder(&self) -> PathBuf {
        self.data_dir.join("sounds")
    }

    pub fn list(&self) -> Vec<SoundEntry> {
        settings::load(&self.data_dir).sound.library
    }

    /// Copies `bytes` in as a new sound; `file_name` only gives the format and the starting label.
    pub fn import(&self, file_name: &str, bytes: &[u8]) -> Result<SoundEntry, SoundError> {
        if bytes.len() > domain::MAX_BYTES {
            return Err(SoundError::TooLarge);
        }
        let format = Format::detect(file_name, bytes).ok_or(SoundError::UnsupportedType)?;
        let entry = SoundEntry {
            id: Uuid::now_v7().to_string(),
            label: domain::label_from_file_name(file_name),
            volume: 100,
            format: format.extension().into(),
        };
        fs::create_dir_all(self.folder())?;
        let path = file_path(&self.folder(), &entry.id, format);
        fs::write(&path, bytes)?;
        if let Err(e) = settings::update(&self.data_dir, |s| s.sound.library.push(entry.clone())) {
            let _ = fs::remove_file(&path);
            return Err(e.into());
        }
        Ok(entry)
    }

    pub fn update(&self, id: &str, label: &str, volume: u8) -> Result<SoundEntry, SoundError> {
        let label = domain::clean_label(label).ok_or(SoundError::Invalid)?;
        if !domain::valid_volume(volume) {
            return Err(SoundError::Invalid);
        }
        let mut found = None;
        settings::update(&self.data_dir, |s| {
            if let Some(e) = s.sound.library.iter_mut().find(|e| e.id == id) {
                e.label = label;
                e.volume = volume;
                found = Some(e.clone());
            }
        })?;
        found.ok_or(SoundError::NotFound)
    }

    /// Removes the sound from the library, then its file.
    pub fn delete(&self, id: &str) -> Result<(), SoundError> {
        let mut removed = None;
        settings::update(&self.data_dir, |s| {
            if let Some(i) = s.sound.library.iter().position(|e| e.id == id) {
                removed = Some(s.sound.library.remove(i));
            }
        })?;
        let entry = removed.ok_or(SoundError::NotFound)?;
        if let Some((path, _)) = self.file_of(&entry)
            && let Err(e) = fs::remove_file(&path)
            && e.kind() != io::ErrorKind::NotFound
        {
            // The library no longer lists it, so nothing plays it; the file only takes space.
            tracing::warn!(code = "sound.file_not_removed", error = %e, "left a sound file behind");
        }
        Ok(())
    }

    /// Where a library sound's file is, and its format.
    pub(crate) fn file(&self, id: &str) -> Option<(PathBuf, Format)> {
        self.list()
            .iter()
            .find(|e| e.id == id)
            .and_then(|e| self.file_of(e))
    }

    /// The path is built from our own UUID and format only, so even an edited `settings.json` cannot point
    /// outside `sounds/`.
    fn file_of(&self, entry: &SoundEntry) -> Option<(PathBuf, Format)> {
        let id = Uuid::try_parse(&entry.id).ok()?;
        let format = Format::from_extension(&entry.format)?;
        Some((file_path(&self.folder(), &id.to_string(), format), format))
    }
}

fn file_path(folder: &Path, id: &str, format: Format) -> PathBuf {
    folder.join(format!("{id}.{}", format.extension()))
}
