//! The background music player. Overlays never report back, so the Host owns the clock: the position follows the
//! track lengths the admin window measured, a track ends when its length has played, and the clock runs only while
//! a music overlay is open in OBS. A reloaded overlay starts from the Host's position.
//!
//! Kept in memory only: after a restart the player is stopped.

use std::{
    future,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use tokio::{
    sync::{Notify, watch},
    time::{Instant, sleep_until},
};
use toolkit_core::{
    modules::{self, Modules},
    settings::{self, BgmSettings},
};

use super::{BgmError, BgmService};
use crate::domain::{self, SceneAction};

/// Pressing "previous" this far into a track starts it over instead.
const RESTART_AFTER_MS: u64 = 3000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlaying {
    pub id: String,
    pub title: String,
    pub duration_ms: u32,
}

/// What the player is doing, as published to the overlays and the admin window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerStatus {
    pub state: PlayState,
    /// The playlist chosen last; kept after a stop so play starts it again.
    pub playlist: Option<String>,
    pub track: Option<NowPlaying>,
    /// Where the track is at the moment of publishing.
    pub position_ms: u64,
    /// A music overlay is open in OBS; without one the clock stands still.
    pub on_air: bool,
    pub shuffle: bool,
    pub volume: u8,
}

/// What a test plays: the track and the music volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestTrack {
    pub track: NowPlaying,
    pub volume: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Starts `playlist` (or the last chosen one) from its start, or resumes it when it is paused.
    Play {
        playlist: Option<String>,
    },
    Pause,
    Next,
    Previous,
    Stop,
    Seek {
        position_ms: u64,
    },
}

struct Inner {
    state: PlayState,
    playlist: Option<String>,
    /// Track IDs in this run's play order.
    order: Vec<String>,
    index: usize,
    track: Option<NowPlaying>,
    /// The position when the clock last stopped or started.
    offset_ms: u64,
    /// When the clock started; `None` while it stands still.
    since: Option<Instant>,
    on_air: bool,
    /// The OBS scene on air last time it changed; `None` while OBS is not connected.
    scene: Option<String>,
}

impl Inner {
    fn position(&self, now: Instant) -> u64 {
        let elapsed = self.since.map_or(0, |s| (now - s).as_millis() as u64);
        let length = self.track.as_ref().map_or(0, |t| u64::from(t.duration_ms));
        (self.offset_ms + elapsed).min(length)
    }

    /// Stops the clock at the current position, then runs it again if the music plays and is on air.
    fn sync(&mut self, now: Instant) {
        self.offset_ms = self.position(now);
        self.since = (self.state == PlayState::Playing && self.on_air && self.track.is_some())
            .then_some(now);
    }

    /// When the track playing ends, if the clock runs.
    fn ends_at(&self) -> Option<Instant> {
        let length = u64::from(self.track.as_ref()?.duration_ms);
        Some(self.since? + Duration::from_millis(length.saturating_sub(self.offset_ms)))
    }

    fn stop(&mut self) {
        self.state = PlayState::Stopped;
        self.track = None;
        self.order.clear();
        self.index = 0;
        self.offset_ms = 0;
        self.since = None;
    }
}

type Publish = Box<dyn Fn(&PlayerStatus) + Send + Sync>;

pub struct BgmPlayer {
    service: Arc<BgmService>,
    modules: Arc<Modules>,
    inner: Mutex<Inner>,
    wake: Notify,
    publish: Publish,
}

impl BgmPlayer {
    /// Starts the clock task; needs a Tokio runtime. `on_air` says whether a music overlay is open, re-read whenever
    /// `changes` ticks; `publish` gets every status, the first one right away.
    pub fn start(
        service: Arc<BgmService>,
        modules: Arc<Modules>,
        on_air: impl Fn() -> bool + Send + Sync + 'static,
        mut changes: watch::Receiver<()>,
        publish: impl Fn(&PlayerStatus) + Send + Sync + 'static,
    ) -> Arc<Self> {
        let player = Arc::new(Self {
            service,
            modules,
            inner: Mutex::new(Inner {
                state: PlayState::Stopped,
                playlist: None,
                order: Vec::new(),
                index: 0,
                track: None,
                offset_ms: 0,
                since: None,
                on_air: on_air(),
                scene: None,
            }),
            wake: Notify::new(),
            publish: Box::new(publish),
        });
        player.publish(&player.lock());
        let clock = player.clone();
        tokio::spawn(async move {
            let mut watching = true;
            loop {
                let ends_at = clock.lock().ends_at();
                let track_end = async {
                    match ends_at {
                        Some(at) => sleep_until(at).await,
                        None => future::pending().await,
                    }
                };
                let overlays = async {
                    match watching {
                        true => changes.changed().await.is_ok(),
                        false => future::pending().await,
                    }
                };
                tokio::select! {
                    () = track_end => clock.track_ended(),
                    () = clock.wake.notified() => {}
                    open = overlays => match open {
                        true => clock.set_on_air(on_air()),
                        false => watching = false,
                    },
                }
            }
        });
        player
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The current status.
    pub fn status(&self) -> PlayerStatus {
        self.status_of(&self.lock(), &settings::load(self.service.data_dir()).bgm)
    }

    fn status_of(&self, inner: &Inner, bgm: &BgmSettings) -> PlayerStatus {
        PlayerStatus {
            state: inner.state,
            playlist: inner.playlist.clone(),
            track: inner.track.clone(),
            position_ms: inner.position(Instant::now()),
            on_air: inner.on_air,
            shuffle: bgm.shuffle,
            volume: bgm.volume.min(100),
        }
    }

    fn publish(&self, inner: &Inner) {
        let status = self.status_of(inner, &settings::load(self.service.data_dir()).bgm);
        (self.publish)(&status);
        // The clock task re-reads when the track ends.
        self.wake.notify_one();
    }

    pub fn command(&self, command: Command) -> Result<(), BgmError> {
        let mut inner = self.lock();
        let now = Instant::now();
        let result = match command {
            Command::Play { playlist } => self.play(&mut inner, playlist, now),
            Command::Pause => {
                if inner.state == PlayState::Playing {
                    inner.state = PlayState::Paused;
                    inner.sync(now);
                }
                Ok(())
            }
            Command::Stop => {
                inner.stop();
                Ok(())
            }
            Command::Next => {
                if inner.track.is_some() {
                    self.advance(&mut inner, now);
                }
                Ok(())
            }
            Command::Previous => {
                if inner.track.is_some() {
                    if inner.position(now) > RESTART_AFTER_MS {
                        inner.offset_ms = 0;
                        inner.since = None;
                        inner.sync(now);
                    } else {
                        let back = inner.index.checked_sub(1).unwrap_or(inner.order.len() - 1);
                        self.start_at(&mut inner, back, false, now);
                    }
                }
                Ok(())
            }
            Command::Seek { position_ms } => {
                if inner.track.is_some() {
                    inner.offset_ms = position_ms;
                    inner.since = None;
                    // Clamped to the length; seeking to the end ends the track.
                    inner.sync(now);
                }
                Ok(())
            }
        };
        self.publish(&inner);
        result
    }

    fn play(
        &self,
        inner: &mut Inner,
        playlist: Option<String>,
        now: Instant,
    ) -> Result<(), BgmError> {
        if !self.modules.is_enabled(modules::BGM) {
            return Err(BgmError::Off);
        }
        let chosen = playlist
            .or_else(|| inner.playlist.clone())
            .ok_or(BgmError::Invalid)?;
        let same = inner.playlist.as_deref() == Some(chosen.as_str()) && inner.track.is_some();
        if same {
            inner.state = PlayState::Playing;
            inner.sync(now);
            return Ok(());
        }
        let bgm = settings::load(self.service.data_dir()).bgm;
        let order = play_order(&bgm, &chosen).ok_or(BgmError::NotFound)?;
        match self.start_playlist(inner, chosen, order, now) {
            true => Ok(()),
            false => Err(BgmError::NothingToPlay),
        }
    }

    /// Plays `playlist` in `order` from its first playable track; stops when none can play.
    fn start_playlist(
        &self,
        inner: &mut Inner,
        playlist: String,
        order: Vec<String>,
        now: Instant,
    ) -> bool {
        inner.stop();
        inner.playlist = Some(playlist);
        inner.order = order;
        inner.state = PlayState::Playing;
        self.start_at(inner, 0, true, now)
    }

    /// Follows the OBS scene on air (`None`: OBS not connected) by the scene rules. With no rules at all the
    /// player is left alone, so it can still be used by hand.
    pub fn scene_changed(&self, scene: Option<String>) {
        let mut inner = self.lock();
        let from = std::mem::replace(&mut inner.scene, scene.clone());
        if domain::same_scene(from.as_deref(), scene.as_deref())
            || !self.modules.is_enabled(modules::BGM)
        {
            return;
        }
        let bgm = settings::load(self.service.data_dir()).bgm;
        if bgm.scene_rules.is_empty() {
            return;
        }
        let now = Instant::now();
        match domain::scene_action(&bgm.scene_rules, from.as_deref(), scene.as_deref()) {
            SceneAction::Keep => return,
            SceneAction::Stop => inner.stop(),
            SceneAction::Play(rule) => {
                let (volume, shuffle) = (rule.volume.min(100), rule.shuffle);
                let Some(playlist) = rule.playlist.clone() else {
                    return;
                };
                if let Err(e) = settings::update(self.service.data_dir(), |s| {
                    s.bgm.volume = volume;
                    s.bgm.shuffle = shuffle;
                }) {
                    // It still plays, at the volume and in the order saved before.
                    tracing::warn!(code = "music.save_failed", error = %e, "scene rule options not saved");
                }
                self.switch_to(&mut inner, playlist, now);
            }
        }
        self.publish(&inner);
    }

    /// Plays `playlist`; the track playing goes on uninterrupted when the playlist has it too.
    fn switch_to(&self, inner: &mut Inner, playlist: String, now: Instant) {
        let bgm = settings::load(self.service.data_dir()).bgm;
        let Some(mut order) = play_order(&bgm, &playlist) else {
            inner.stop();
            return;
        };
        let playing = inner.track.as_ref().map(|t| t.id.clone());
        match playing.filter(|id| order.contains(id)) {
            Some(id) => {
                inner.index = order_around(&mut order, &id, bgm.shuffle);
                inner.order = order;
                inner.playlist = Some(playlist);
                inner.state = PlayState::Playing;
                inner.sync(now);
            }
            None => {
                self.start_playlist(inner, playlist, order, now);
            }
        }
    }

    /// Plays the first playable track from `index` on (backwards unless `forward`), wrapping around; stops when
    /// none can play. Keeps playing or paused as it was.
    fn start_at(&self, inner: &mut Inner, index: usize, forward: bool, now: Instant) -> bool {
        let bgm = settings::load(self.service.data_dir()).bgm;
        let n = inner.order.len();
        for k in 0..n {
            let i = match forward {
                true => (index + k) % n,
                false => (index + n - k % n) % n,
            };
            if let Some(track) = self.playable(&bgm, &inner.order[i]) {
                inner.index = i;
                inner.track = Some(track);
                inner.offset_ms = 0;
                inner.since = None;
                inner.sync(now);
                return true;
            }
        }
        inner.stop();
        false
    }

    /// A track the player can play: in the library, its file there and its length measured.
    fn playable(&self, bgm: &BgmSettings, id: &str) -> Option<NowPlaying> {
        let entry = bgm.library.iter().find(|e| e.id == id)?;
        let duration_ms = entry.duration_ms.filter(|ms| *ms > 0)?;
        (!self.service.file_missing(entry)).then(|| NowPlaying {
            id: entry.id.clone(),
            title: entry.title.clone(),
            duration_ms,
        })
    }

    /// The next track; after the last one the playlist starts over in a fresh order, picking up edits.
    fn advance(&self, inner: &mut Inner, now: Instant) {
        let next = inner.index + 1;
        if next < inner.order.len() {
            self.start_at(inner, next, true, now);
            return;
        }
        let bgm = settings::load(self.service.data_dir()).bgm;
        match inner.playlist.as_deref().and_then(|p| play_order(&bgm, p)) {
            Some(order) if !order.is_empty() => {
                inner.order = order;
                self.start_at(inner, 0, true, now);
            }
            _ => inner.stop(),
        }
    }

    fn track_ended(&self) {
        let mut inner = self.lock();
        let now = Instant::now();
        // Woken late, or the track changed meanwhile: only a track that has played to its end moves on.
        if inner.ends_at().is_none_or(|at| at > now) {
            return;
        }
        // Turned off while it played: the music stops at the end of the track.
        if self.modules.is_enabled(modules::BGM) {
            self.advance(&mut inner, now);
        } else {
            inner.stop();
        }
        self.publish(&inner);
    }

    fn set_on_air(&self, on_air: bool) {
        let mut inner = self.lock();
        if inner.on_air == on_air {
            return;
        }
        inner.on_air = on_air;
        inner.sync(Instant::now());
        self.publish(&inner);
    }

    /// Hands `send` the track a test plays: the first playable one of the playlist chosen last, in the playlist's own
    /// order. Changes nothing; refused while the music plays. `send` runs under the player's lock, so a play command
    /// cannot slip in between the check and the test: the overlay always hears the test before the music starts.
    pub fn test(&self, send: impl FnOnce(TestTrack)) -> Result<(), BgmError> {
        if !self.modules.is_enabled(modules::BGM) {
            return Err(BgmError::Off);
        }
        let inner = self.lock();
        if inner.state == PlayState::Playing {
            return Err(BgmError::Playing);
        }
        let bgm = settings::load(self.service.data_dir()).bgm;
        let track = inner
            .playlist
            .as_deref()
            .and_then(|id| bgm.playlists.iter().find(|p| p.id == id))
            .and_then(|p| p.tracks.iter().find_map(|t| self.playable(&bgm, t)))
            .ok_or(BgmError::NothingToPlay)?;
        send(TestTrack {
            track,
            volume: bgm.volume.min(100),
        });
        Ok(())
    }

    /// Saves the volume and the order; turning shuffle on or off reorders the rest of the run around the track
    /// playing.
    pub fn set_options(&self, volume: u8, shuffle: bool) -> Result<(), BgmError> {
        if volume > 100 {
            return Err(BgmError::Invalid);
        }
        let mut inner = self.lock();
        let mut changed = false;
        settings::update(self.service.data_dir(), |s| {
            changed = s.bgm.shuffle != shuffle;
            s.bgm.volume = volume;
            s.bgm.shuffle = shuffle;
        })?;
        let bgm = settings::load(self.service.data_dir()).bgm;
        if changed
            && let Some(track) = inner.track.clone()
            && let Some(mut order) = inner.playlist.as_deref().and_then(|p| play_order(&bgm, p))
        {
            inner.index = order_around(&mut order, &track.id, shuffle);
            inner.order = order;
        }
        self.publish(&inner);
        Ok(())
    }
}

/// Where `id` is in `order` from now on: in order, where the playlist has it; shuffled (or not in the playlist), moved
/// first with the rest after it.
fn order_around(order: &mut Vec<String>, id: &str, shuffle: bool) -> usize {
    match order.iter().position(|t| t == id) {
        Some(i) if !shuffle => i,
        _ => {
            order.retain(|t| t != id);
            order.insert(0, id.to_owned());
            0
        }
    }
}

/// A playlist's track IDs in play order: its own, or shuffled. `None` when there is no such playlist.
fn play_order(bgm: &BgmSettings, playlist: &str) -> Option<Vec<String>> {
    let mut order = bgm
        .playlists
        .iter()
        .find(|p| p.id == playlist)?
        .tracks
        .clone();
    if bgm.shuffle {
        fastrand::shuffle(&mut order);
    }
    Some(order)
}

#[cfg(test)]
mod tests;
