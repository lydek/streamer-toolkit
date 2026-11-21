//! Screen tool use cases: the break card's settings, showing and hiding it, and the on-stream warnings. Every change
//! is announced with the whole card, or all the warnings.

use std::{future::Future, sync::Mutex};

use chrono::{DateTime, Utc};

use crate::domain::{
    BreakCard, BreakSettings, Countdown,
    warnings::{Conditions, WarningSettings, Warnings},
};

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
}

/// Announces the card after every change and once at start.
pub type PublishBreak = Box<dyn Fn(&BreakCard) + Send + Sync>;
/// Announces the warnings whenever their settings or state change, and once at start.
pub type PublishWarnings = Box<dyn Fn(&WarningSettings, &Warnings) + Send + Sync>;

#[derive(Default)]
struct WarningState {
    settings: WarningSettings,
    conditions: Conditions,
    warnings: Warnings,
}

pub struct OverlayService<R> {
    repo: R,
    publish: PublishBreak,
    publish_warnings: PublishWarnings,
    /// One change at a time: each reads, changes and saves a single row.
    lock: tokio::sync::Mutex<()>,
    warnings: Mutex<WarningState>,
}

impl<R: OverlayRepository> OverlayService<R> {
    pub(crate) fn new(repo: R, publish: PublishBreak, publish_warnings: PublishWarnings) -> Self {
        Self {
            repo,
            publish,
            publish_warnings,
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
        Ok(())
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
