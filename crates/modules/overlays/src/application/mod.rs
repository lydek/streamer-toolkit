//! Screen tool use cases: the break card's settings, showing and hiding it. Every change is announced with the
//! whole card.

use std::future::Future;

use chrono::{DateTime, Utc};

use crate::domain::{BreakCard, BreakSettings, Countdown};

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
}

/// Announces the card after every change and once at start.
pub type PublishBreak = Box<dyn Fn(&BreakCard) + Send + Sync>;

pub struct OverlayService<R> {
    repo: R,
    publish: PublishBreak,
    /// One change at a time: each reads, changes and saves the single row.
    lock: tokio::sync::Mutex<()>,
}

impl<R: OverlayRepository> OverlayService<R> {
    pub(crate) fn new(repo: R, publish: PublishBreak) -> Self {
        Self {
            repo,
            publish,
            lock: tokio::sync::Mutex::new(()),
        }
    }

    pub async fn break_card(&self) -> Result<BreakCard, RepositoryError> {
        Ok(self.repo.break_card().await?.unwrap_or_default())
    }

    /// Announces the stored card, so a restart shows the same countdown.
    pub(crate) async fn announce(&self) -> Result<(), RepositoryError> {
        let card = self.break_card().await?;
        (self.publish)(&card);
        Ok(())
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
