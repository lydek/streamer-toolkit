//! Screen tool use cases: the break card's settings, showing and hiding it, and the on-stream warnings. Every change
//! is announced with the whole card, or all the warnings.

use std::{future::Future, path::PathBuf, sync::Mutex};

use chrono::{DateTime, Utc};

use crate::domain::{
    BreakCard, BreakSettings, Countdown,
    appearance::Appearance,
    media::{MediaEntry, MediaRule},
    warnings::{Conditions, WarningSettings, Warnings},
};

mod media;
mod playback;

pub use media::MediaError;
use playback::Playback;
pub use playback::{MediaShow, PlaybackDeps, PublishMedia, RecentMedia};

#[derive(Debug, thiserror::Error)]
#[error("overlays.storage_failed: {0}")]
pub struct RepositoryError(pub(crate) String);

#[derive(Debug, thiserror::Error)]
pub enum OverlayError {
    /// A setting or countdown out of range.
    #[error("overlays.invalid")]
    Invalid,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

pub trait OverlayRepository: Send + Sync + 'static {
    /// `None` before anything was saved.
    fn break_card(&self)
    -> impl Future<Output = Result<Option<BreakCard>, RepositoryError>> + Send;
    fn save_break_card(
        &self,
        card: &BreakCard,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
    /// `None` before anything was saved.
    fn warning_settings(
        &self,
    ) -> impl Future<Output = Result<Option<WarningSettings>, RepositoryError>> + Send;
    fn save_warning_settings(
        &self,
        settings: &WarningSettings,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
    /// `None` before anything was saved.
    fn appearance(
        &self,
    ) -> impl Future<Output = Result<Option<Appearance>, RepositoryError>> + Send;
    fn save_appearance(
        &self,
        appearance: Appearance,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;

    /// In import order.
    fn media(&self) -> impl Future<Output = Result<Vec<MediaEntry>, RepositoryError>> + Send;
    fn media_entry(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<Option<MediaEntry>, RepositoryError>> + Send;
    fn insert_media(
        &self,
        entry: &MediaEntry,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
    /// Title and length.
    fn update_media(
        &self,
        entry: &MediaEntry,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
    /// The entry and the rules that show it.
    fn delete_media(&self, id: &str) -> impl Future<Output = Result<(), RepositoryError>> + Send;

    fn media_rules(&self) -> impl Future<Output = Result<Vec<MediaRule>, RepositoryError>> + Send;
    fn save_media_rule(
        &self,
        rule: &MediaRule,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
    /// `false` when the reward has no rule.
    fn delete_media_rule(
        &self,
        reward_id: &str,
    ) -> impl Future<Output = Result<bool, RepositoryError>> + Send;
}

/// Announces the card after every change and once at start.
pub type PublishBreak = Box<dyn Fn(&BreakCard) + Send + Sync>;
/// Announces the warnings whenever their settings or state change, and once at start.
pub type PublishWarnings = Box<dyn Fn(&WarningSettings, &Warnings) + Send + Sync>;
/// Announces the built-in overlays' look after every change and once at start.
pub type PublishAppearance = Box<dyn Fn(Appearance) + Send + Sync>;

/// The announcers, one per event.
pub struct Publishers {
    pub(crate) break_card: PublishBreak,
    pub(crate) warnings: PublishWarnings,
    pub(crate) appearance: PublishAppearance,
    pub(crate) media: PublishMedia,
}

#[derive(Default)]
struct WarningState {
    settings: WarningSettings,
    conditions: Conditions,
    warnings: Warnings,
}

pub struct OverlayService<R> {
    repo: R,
    /// The data folder's `media/`.
    media_dir: PathBuf,
    publish: PublishBreak,
    publish_warnings: PublishWarnings,
    publish_appearance: PublishAppearance,
    publish_media: PublishMedia,
    playback: Playback,
    /// One change at a time: each reads, changes and saves a single row.
    lock: tokio::sync::Mutex<()>,
    warnings: Mutex<WarningState>,
}

impl<R: OverlayRepository> OverlayService<R> {
    pub(crate) fn new(
        repo: R,
        media_dir: PathBuf,
        publishers: Publishers,
        deps: PlaybackDeps,
    ) -> Self {
        Self {
            repo,
            media_dir,
            publish: publishers.break_card,
            publish_warnings: publishers.warnings,
            publish_appearance: publishers.appearance,
            publish_media: publishers.media,
            playback: Playback::new(deps),
            lock: tokio::sync::Mutex::new(()),
            warnings: Mutex::default(),
        }
    }

    pub async fn break_card(&self) -> Result<BreakCard, RepositoryError> {
        Ok(self.repo.break_card().await?.unwrap_or_default())
    }

    /// Announces the stored card, so a restart shows the same countdown, and the warnings with their settings.
    pub(crate) async fn announce(&self) -> Result<(), RepositoryError> {
        let card = self.break_card().await?;
        (self.publish)(&card);
        let settings = self.repo.warning_settings().await?.unwrap_or_default();
        self.apply_warning_settings(settings, Utc::now());
        (self.publish_appearance)(self.appearance().await?);
        self.reload_library().await
    }

    pub async fn appearance(&self) -> Result<Appearance, RepositoryError> {
        Ok(self.repo.appearance().await?.unwrap_or_default())
    }

    pub async fn set_appearance(&self, appearance: Appearance) -> Result<Appearance, OverlayError> {
        if !appearance.valid() {
            return Err(OverlayError::Invalid);
        }
        let _one = self.lock.lock().await;
        self.repo.save_appearance(appearance).await?;
        (self.publish_appearance)(appearance);
        Ok(appearance)
    }

    /// The warning settings and state as of the last conditions.
    pub fn warnings(&self) -> (WarningSettings, Warnings) {
        let w = self.warnings.lock().unwrap();
        (w.settings.clone(), w.warnings.clone())
    }

    /// Takes what the host saw at `now`; announces only a change.
    pub fn see(&self, conditions: Conditions, now: DateTime<Utc>) {
        let mut w = self.warnings.lock().unwrap();
        let next = w.warnings.next(&w.settings, &conditions, now);
        w.conditions = conditions;
        if next != w.warnings {
            w.warnings = next;
            (self.publish_warnings)(&w.settings, &w.warnings);
        }
    }

    fn apply_warning_settings(&self, settings: WarningSettings, now: DateTime<Utc>) {
        let mut w = self.warnings.lock().unwrap();
        // Another input starts its own wait.
        if settings.mic_input != w.settings.mic_input {
            w.warnings.mic_muted_since = None;
        }
        w.warnings = w.warnings.next(&settings, &w.conditions, now);
        w.settings = settings;
        (self.publish_warnings)(&w.settings, &w.warnings);
    }

    pub async fn set_warning_settings(
        &self,
        settings: WarningSettings,
        now: DateTime<Utc>,
    ) -> Result<(WarningSettings, Warnings), OverlayError> {
        let settings = settings.normalized().ok_or(OverlayError::Invalid)?;
        let _one = self.lock.lock().await;
        self.repo.save_warning_settings(&settings).await?;
        self.apply_warning_settings(settings, now);
        Ok(self.warnings())
    }

    async fn change(
        &self,
        f: impl FnOnce(&mut BreakCard) -> Option<()>,
    ) -> Result<BreakCard, OverlayError> {
        let _one = self.lock.lock().await;
        let mut card = self.break_card().await?;
        f(&mut card).ok_or(OverlayError::Invalid)?;
        self.repo.save_break_card(&card).await?;
        (self.publish)(&card);
        Ok(card)
    }

    /// Saves the settings; a shown card keeps its countdown and shows the new texts.
    pub async fn set_break_settings(
        &self,
        settings: BreakSettings,
    ) -> Result<BreakCard, OverlayError> {
        let settings = settings.normalized().ok_or(OverlayError::Invalid)?;
        self.change(|card| {
            card.settings = settings;
            Some(())
        })
        .await
    }

    pub async fn show_break(
        &self,
        countdown: Countdown,
        now: DateTime<Utc>,
    ) -> Result<BreakCard, OverlayError> {
        self.change(|card| card.show(countdown, now)).await
    }

    pub async fn hide_break(&self) -> Result<BreakCard, OverlayError> {
        self.change(|card| {
            card.hide();
            Some(())
        })
        .await
    }
}
