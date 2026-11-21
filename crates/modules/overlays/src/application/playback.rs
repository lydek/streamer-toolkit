//! Showing a reward's video or GIF: each new redemption of a reward with a rule queues one item on the presentation
//! queue, shown on the alerts overlay when its turn comes. The rules and the library are kept in memory, so a
//! redemption queues in the same step it arrives.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use toolkit_core::{
    events::{ChannelPointsRedemption, RedemptionChange, Source},
    modules::{self, Modules},
    presentation::{Item, Presenter, Recent, RecentItem, Shown, Stage, Subject},
};

use super::{OverlayRepository, OverlayService, RepositoryError, media::MediaError};
use crate::domain::media::{MediaEntry, MediaFormat, MediaRule, Position};

/// What the alerts overlay shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaShow {
    pub media_id: String,
    pub format: MediaFormat,
    pub position: Position,
    /// Percent, as it plays (the sound master volume unless the rule sets its own).
    pub volume: u8,
    pub duration_ms: u32,
}

/// Publishes a show to the alerts overlay; `infrastructure` puts it on the bus.
pub type PublishMedia = Arc<dyn Fn(&MediaShow, Source) + Send + Sync>;
/// The sound module's master volume, in percent.
pub type MasterVolume = Box<dyn Fn() -> u8 + Send + Sync>;

/// What showing media needs from the rest of the app.
pub struct PlaybackDeps {
    pub(crate) presenter: Arc<Presenter>,
    pub(crate) modules: Arc<Modules>,
    pub(crate) master_volume: MasterVolume,
}

/// How many redemptions declined for a missing file are remembered, as many as [`Recent`] keeps.
const DECLINED_KEEP: usize = Recent::KEEP;

#[derive(Default)]
pub(crate) struct Library {
    pub(crate) entries: Vec<MediaEntry>,
    pub(crate) rules: Vec<MediaRule>,
}

pub(crate) struct Playback {
    pub(crate) deps: PlaybackDeps,
    pub(crate) library: Mutex<Library>,
    recent: Recent,
    /// Redemption IDs whose item was declined because its file is gone.
    missing: Arc<Mutex<VecDeque<String>>>,
}

impl Playback {
    pub(crate) fn new(deps: PlaybackDeps) -> Self {
        Self {
            deps,
            library: Mutex::default(),
            recent: Recent::default(),
            missing: Arc::default(),
        }
    }
}

/// A queued media item of this launch and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentMedia {
    pub item: RecentItem,
    /// Declined because its file is gone.
    pub file_missing: bool,
}

impl<R: OverlayRepository> OverlayService<R> {
    /// Reads the library and the rules into memory again; after every change to them.
    pub(crate) async fn reload_library(&self) -> Result<(), RepositoryError> {
        let entries = self.repo.media().await?;
        let rules = self.repo.media_rules().await?;
        *self.playback.library.lock().unwrap() = Library { entries, rules };
        Ok(())
    }

    pub fn media_rules(&self) -> Vec<MediaRule> {
        self.playback.library.lock().unwrap().rules.clone()
    }

    /// Sets what `rule.reward_id` shows; the media item must be in the library.
    pub async fn set_media_rule(&self, rule: MediaRule) -> Result<MediaRule, MediaError> {
        if !rule.valid() {
            return Err(MediaError::Invalid);
        }
        let _one = self.lock.lock().await;
        if self.repo.media_entry(&rule.media_id).await?.is_none() {
            return Err(MediaError::NotFound);
        }
        self.repo.save_media_rule(&rule).await?;
        self.reload_library().await?;
        Ok(rule)
    }

    pub async fn delete_media_rule(&self, reward_id: &str) -> Result<(), MediaError> {
        let _one = self.lock.lock().await;
        if !self.repo.delete_media_rule(reward_id).await? {
            return Err(MediaError::NotFound);
        }
        self.reload_library().await?;
        Ok(())
    }

    /// The show for `reward_id`'s rule; `None` without a rule, an item in the library, or a length yet.
    fn show_for(&self, reward_id: &str) -> Option<(MediaShow, String)> {
        let library = self.playback.library.lock().unwrap();
        let rule = library.rules.iter().find(|r| r.reward_id == reward_id)?;
        let entry = library.entries.iter().find(|e| e.id == rule.media_id)?;
        let volume = rule
            .volume
            .unwrap_or_else(|| (self.playback.deps.master_volume)());
        Some((
            MediaShow {
                media_id: entry.id.clone(),
                format: entry.format,
                position: rule.position,
                volume,
                duration_ms: entry.duration_ms?,
            },
            entry.title.clone(),
        ))
    }

    /// Queues the reward's video or GIF for a new redemption, for as long as it shows. Kept in memory only, so
    /// redemptions from before a restart never show late.
    pub fn receive_redemption(&self, r: &ChannelPointsRedemption) {
        let deps = &self.playback.deps;
        if r.change != RedemptionChange::Added || !deps.modules.is_enabled(modules::SCREEN_TOOLS) {
            return;
        }
        let Some((show, title)) = self.show_for(&r.reward_id) else {
            return;
        };
        let subject = Subject::Media {
            media: title,
            viewer: r.user_name.clone(),
        };
        let done = self
            .playback
            .recent
            .record(Some(r.id.clone()), subject.clone(), Source::Twitch);
        let (publish, modules, missing, key) = (
            self.publish_media.clone(),
            deps.modules.clone(),
            self.playback.missing.clone(),
            r.id.clone(),
        );
        let file = self
            .media_dir
            .join(format!("{}.{}", show.media_id, show.format.extension()));
        deps.presenter.push(Item {
            stage: Stage::Alerts,
            subject,
            show: Duration::from_millis(show.duration_ms.into()),
            simulated: false,
            ready: None,
            present: Box::new(move || {
                // Turned off while it waited: it does not show.
                if !modules.is_enabled(modules::SCREEN_TOOLS) {
                    return Shown::No;
                }
                // Its file is gone: skipped at once, so it holds up nothing behind it.
                if !file.is_file() {
                    let mut gone = missing.lock().unwrap();
                    gone.push_front(key);
                    gone.truncate(DECLINED_KEEP);
                    return Shown::No;
                }
                publish(&show, Source::Twitch);
                Shown::Yes
            }),
            done: Some(done),
        });
    }

    /// 試播: shows the reward's media on the alerts overlay right away, as `source=simulation`, without the
    /// presentation queue; nothing is recorded.
    pub fn test_media(&self, reward_id: &str) -> Result<(), MediaError> {
        let (show, _) = self.show_for(reward_id).ok_or(MediaError::NotFound)?;
        (self.publish_media)(&show, Source::Simulation);
        Ok(())
    }

    /// The latest media items queued (this launch only), newest first, with what became of each.
    pub fn media_recent(&self) -> Vec<RecentMedia> {
        let missing = self.playback.missing.lock().unwrap();
        self.playback
            .recent
            .list()
            .into_iter()
            .map(|item| RecentMedia {
                file_missing: item.key.as_ref().is_some_and(|k| missing.contains(k)),
                item,
            })
            .collect()
    }
}
