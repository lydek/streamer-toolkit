//! The sound library: imported files are copied into the data folder's `sounds/` under a UUID v7 name, and their
//! label and volume live in `settings.json`.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use toolkit_core::{
    modules::{self, Modules},
    settings::{self, SoundEntry},
};
use uuid::Uuid;

use crate::domain::{self, Cue, Format, SILENT};

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
    #[error("sound.unknown_slot")]
    UnknownSlot,
    #[error("sound.save_failed")]
    SaveFailed(#[from] io::Error),
}

pub struct SoundService {
    data_dir: PathBuf,
    modules: Arc<Modules>,
}

impl SoundService {
    pub fn new(data_dir: PathBuf, modules: Arc<Modules>) -> Self {
        Self { data_dir, modules }
    }

    fn folder(&self) -> PathBuf {
        self.data_dir.join("sounds")
    }

    pub fn list(&self) -> Vec<SoundEntry> {
        settings::load(&self.data_dir).sound.library
    }

    /// Chosen slots: slot → a library sound's ID or `silent`.
    pub fn slots(&self) -> BTreeMap<String, String> {
        settings::load(&self.data_dir).sound.slots
    }

    /// Chooses what `slot` plays: a library sound's ID, `silent`, or `None` for its default.
    pub fn set_slot(&self, slot: &str, sound: Option<&str>) -> Result<(), SoundError> {
        if !domain::valid_slot(slot) {
            return Err(SoundError::UnknownSlot);
        }
        let mut known = true;
        settings::update(&self.data_dir, |s| match sound {
            None => {
                s.sound.slots.remove(slot);
            }
            Some(id) if id == SILENT || s.sound.library.iter().any(|e| e.id == id) => {
                s.sound.slots.insert(slot.into(), id.into());
            }
            Some(_) => known = false,
        })?;
        known.then_some(()).ok_or(SoundError::NotFound)
    }

    /// What `slot` plays now. While the module is off every slot plays its default.
    pub fn cue(&self, slot: &str) -> Cue {
        if !self.modules.is_enabled(modules::SOUND) {
            return domain::default_cue(slot);
        }
        let sound = settings::load(&self.data_dir).sound;
        match sound.slots.get(slot).map(String::as_str) {
            None => domain::default_cue(slot),
            Some(SILENT) => Cue::Silent,
            // A sound deleted outside the app also falls back to the default.
            Some(id) => sound.library.into_iter().find(|e| e.id == id).map_or_else(
                || domain::default_cue(slot),
                |e| Cue::File {
                    id: e.id,
                    volume: e.volume,
                },
            ),
        }
    }

    /// `kind` as in the `alert` payload, e.g. `follow`.
    pub fn alert(&self, kind: &str) -> Cue {
        self.cue(&format!("alert.{kind}"))
    }

    /// A normal check-in, or one that filled the card.
    pub fn checkin(&self, full: bool) -> Cue {
        self.cue(if full { "checkin.full" } else { "checkin" })
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

    /// Removes the sound from the library and the slots that play it, then its file.
    pub fn delete(&self, id: &str) -> Result<(), SoundError> {
        let mut removed = None;
        settings::update(&self.data_dir, |s| {
            if let Some(i) = s.sound.library.iter().position(|e| e.id == id) {
                removed = Some(s.sound.library.remove(i));
                // Slots that played it go back to their defaults.
                s.sound.slots.retain(|_, chosen| chosen != id);
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
