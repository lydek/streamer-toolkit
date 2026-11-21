//! The sound library: imported files are copied into the data folder's `sounds/` under a UUID v7 name, and their
//! label and volume live in `settings.json`.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use toolkit_core::{
    events::{ChannelPointsRedemption, RedemptionChange, Source},
    modules::{self, Modules},
    presentation::{Item, Presenter, Recent, RecentItem, Shown, Stage, Subject},
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

/// Plays a library sound (ID, volume) on `/overlay/sound/`; `infrastructure` publishes it on the bus.
type Play = Arc<dyn Fn(&str, u8, Source) + Send + Sync>;
/// Silences the test sounds on `/overlay/sound/`; `infrastructure` publishes it on the bus.
type StopTests = Arc<dyn Fn() + Send + Sync>;

/// How long a reward sound holds the presentation queue before the admin window has measured it.
const UNMEASURED: Duration = Duration::from_secs(5);

/// The longest length the admin window may report: 6 hours, more than any file within the 10 MB import limit can
/// hold even at a low bitrate, so every importable sound can have its length.
const DURATION_MAX_MS: u32 = 6 * 60 * 60 * 1000;

pub struct SoundService {
    data_dir: PathBuf,
    modules: Arc<Modules>,
    presenter: Arc<Presenter>,
    play: Play,
    stop_tests: StopTests,
    /// What became of the latest reward sounds.
    recent: Recent,
}

impl SoundService {
    pub(crate) fn with_player(
        data_dir: PathBuf,
        modules: Arc<Modules>,
        presenter: Arc<Presenter>,
        play: impl Fn(&str, u8, Source) + Send + Sync + 'static,
        stop_tests: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self {
            data_dir,
            modules,
            presenter,
            play: Arc::new(play),
            stop_tests: Arc::new(stop_tests),
            recent: Recent::default(),
        }
    }

    /// Queues the sound chosen for a new redemption's reward on the presentation queue, for as long as it plays;
    /// `/overlay/sound/` plays it when its turn comes. Kept in memory only, so redemptions from before a restart
    /// never play late.
    pub fn receive_redemption(&self, r: &ChannelPointsRedemption) {
        if r.change != RedemptionChange::Added || !self.modules.is_enabled(modules::SOUND) {
            return;
        }
        let Cue::File { id, volume } = self.cue(&format!("reward.{}", r.reward_id)) else {
            return;
        };
        let entry = self.list().into_iter().find(|e| e.id == id);
        let show = entry
            .as_ref()
            .and_then(|e| e.duration_ms)
            .map_or(UNMEASURED, |ms| Duration::from_millis(ms.into()));
        let (play, modules, missing) = (
            self.play.clone(),
            self.modules.clone(),
            self.file_missing(&id),
        );
        let subject = Subject::Sound {
            sound: entry.map(|e| e.label).unwrap_or_default(),
            viewer: r.user_name.clone(),
        };
        let done = self
            .recent
            .record(Some(r.id.clone()), subject.clone(), Source::Twitch);
        self.presenter.push(Item {
            stage: Stage::Sound,
            subject,
            show,
            simulated: false,
            ready: None,
            present: Box::new(move || {
                // Turned off while it waited: it does not play.
                if !modules.is_enabled(modules::SOUND) {
                    return Shown::No;
                }
                // Its file is gone: nothing to hear, but it keeps its turn so the queue area shows why.
                if missing {
                    return Shown::WithoutSound;
                }
                play(&id, volume, Source::Twitch);
                Shown::Yes
            }),
            done: Some(done),
        });
    }

    /// The latest reward sounds queued (this launch only), newest first, with what became of each.
    pub fn recent(&self) -> Vec<RecentItem> {
        self.recent.list()
    }

    /// The test button: plays a library sound on `/overlay/sound/` right away, as `source=simulation`, without
    /// waiting for the presentation queue; nothing is recorded.
    pub fn play_test(&self, id: &str) -> Result<(), SoundError> {
        let entry = self
            .list()
            .into_iter()
            .find(|e| e.id == id)
            .ok_or(SoundError::NotFound)?;
        let volume = domain::scaled(entry.volume, self.volume());
        (self.play)(&entry.id, volume, Source::Simulation);
        Ok(())
    }

    /// Stops what the test button sent: the test sound playing and the waiting ones. Reward sounds play on.
    pub fn stop_tests(&self) {
        (self.stop_tests)();
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

    /// What `slot` plays now, at the master volume. While the module is off every slot plays its default.
    pub fn cue(&self, slot: &str) -> Cue {
        let sound = settings::load(&self.data_dir).sound;
        let cue = match sound.slots.get(slot).map(String::as_str) {
            _ if !self.modules.is_enabled(modules::SOUND) => domain::default_cue(slot),
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
        };
        cue.at(sound.volume)
    }

    /// The master volume, 0–100.
    pub fn volume(&self) -> u8 {
        settings::load(&self.data_dir).sound.volume.min(100)
    }

    pub fn set_volume(&self, volume: u8) -> Result<(), SoundError> {
        if !domain::valid_volume(volume) {
            return Err(SoundError::Invalid);
        }
        settings::update(&self.data_dir, |s| s.sound.volume = volume)?;
        Ok(())
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
            label: toolkit_core::audio::label_from_file_name(file_name, "sound"),
            volume: 100,
            format: format.extension().into(),
            duration_ms: None,
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

    /// Records how long a sound plays, as the admin window measured it (1 ms to 6 hours).
    pub fn set_duration(&self, id: &str, duration_ms: u32) -> Result<(), SoundError> {
        if !(1..=DURATION_MAX_MS).contains(&duration_ms) {
            return Err(SoundError::Invalid);
        }
        let mut found = false;
        settings::update(&self.data_dir, |s| {
            if let Some(e) = s.sound.library.iter_mut().find(|e| e.id == id) {
                e.duration_ms = Some(duration_ms);
                found = true;
            }
        })?;
        found.then_some(()).ok_or(SoundError::NotFound)
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

    /// The library lists it but its file is gone from the sounds folder (deleted outside the app).
    pub fn file_missing(&self, id: &str) -> bool {
        !self.file(id).is_some_and(|(path, _)| path.is_file())
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
