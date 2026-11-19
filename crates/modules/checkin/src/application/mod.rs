//! Check-in use cases: a chat command checks a viewer in, once per business day. Check-ins are handled one at a
//! time, in arrival order; the repository also refuses a repeat on its own.

use std::{
    future::Future,
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};

use chrono::{DateTime, NaiveDate, Utc};
use tokio::sync::{mpsc, oneshot};
use toolkit_core::{
    events::{ChatMessage, Source},
    modules::{self, Modules},
    presentation::{Item, Presenter, Shown, Stage, Subject},
    settings::{self, CheckinSettings},
};

use crate::domain::{
    BusinessDay, CardSlots, CheckIn, Command, ImportedViewer, InvalidSettings, RewardId, Rules,
    Trigger, TwitchUserId, Viewer,
};

#[derive(Debug, thiserror::Error)]
#[error("checkin.storage_failed: {0}")]
pub(crate) struct RepositoryError(pub(crate) String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Recorded; the viewer's internal ID and total check-ins after it.
    CheckedIn { viewer_id: String, total: u64 },
    /// The viewer already checked in on that business day; nothing was written.
    Repeat,
}

/// Storage port; the SQLite implementation lives in `infrastructure::db`.
pub(crate) trait CheckinRepository: Send + Sync + 'static {
    /// Twitch user IDs of every known viewer.
    fn known_viewers(
        &self,
    ) -> impl Future<Output = Result<std::collections::HashSet<String>, RepositoryError>> + Send;

    /// In one transaction: sets each viewer's imported baseline (adding unknown viewers), keeping names and
    /// profile images already known.
    fn import(
        &self,
        viewers: &[ImportedViewer],
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;

    /// The viewer's stored profile image URL.
    fn profile_image(
        &self,
        viewer_id: &str,
    ) -> impl Future<Output = Result<Option<String>, RepositoryError>> + Send;

    fn set_profile_image(
        &self,
        viewer_id: &str,
        url: &str,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;

    /// A viewer by internal ID.
    fn find_viewer(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<Option<Viewer>, RepositoryError>> + Send;

    /// In one transaction: keeps the viewer's latest names, and records `check_in` unless the viewer already has a
    /// check-in labelled `check_in.day` or happening inside `check_in.window`.
    fn check_in(
        &self,
        viewer: &Viewer,
        check_in: &CheckIn,
    ) -> impl Future<Output = Result<Outcome, RepositoryError>> + Send;

    /// [`CheckinRepository::check_in`] for the outbox event `event_id`, recording the ID in the inbox in the same
    /// transaction. An event already in the inbox changes nothing and is a repeat.
    fn check_in_once(
        &self,
        event_id: &str,
        viewer: &Viewer,
        check_in: &CheckIn,
    ) -> impl Future<Output = Result<Outcome, RepositoryError>> + Send;
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ConfigError {
    #[error(transparent)]
    Invalid(#[from] InvalidSettings),
    #[error("settings.save_failed")]
    SaveFailed(#[from] std::io::Error),
}

/// The check-in settings in force, kept in `settings.json` and in memory so chat never waits on the file.
pub(crate) struct Config {
    data_dir: PathBuf,
    rules: RwLock<Rules>,
}

impl Config {
    /// Values in `settings.json` that cannot be used fall back to their defaults one by one.
    pub(crate) fn load(data_dir: PathBuf) -> Self {
        let rules = rules_or_defaults(&settings::load(&data_dir).checkin);
        Self {
            data_dir,
            rules: RwLock::new(rules),
        }
    }

    pub(crate) fn rules(&self) -> Rules {
        self.rules.read().unwrap().clone()
    }

    /// Validates, saves, then applies; nothing changes when either step fails.
    pub(crate) fn update(&self, next: &CheckinSettings) -> Result<Rules, ConfigError> {
        let rules = rules(next)?;
        let mut current = self.rules.write().unwrap();
        settings::update(&self.data_dir, |s| s.checkin = settings_of(&rules))?;
        *current = rules.clone();
        Ok(rules)
    }
}

pub(crate) fn rules(s: &CheckinSettings) -> Result<Rules, InvalidSettings> {
    Ok(Rules {
        command: Command::new(&s.command)?,
        reward: s.reward_id.as_deref().map(RewardId::new).transpose()?,
        day: BusinessDay::new(&s.time_zone, s.reset_hour)?,
        slots: CardSlots::new(s.card_slots)?,
    })
}

fn rules_or_defaults(s: &CheckinSettings) -> Rules {
    let d = CheckinSettings::default();
    Rules {
        command: Command::new(&s.command).unwrap_or_else(|_| Command::new(&d.command).unwrap()),
        reward: s.reward_id.as_deref().and_then(|id| RewardId::new(id).ok()),
        day: BusinessDay::new(&s.time_zone, s.reset_hour)
            .or_else(|_| BusinessDay::new(&s.time_zone, d.reset_hour))
            .unwrap_or_else(|_| BusinessDay::new(&d.time_zone, d.reset_hour).unwrap()),
        slots: CardSlots::new(s.card_slots)
            .unwrap_or_else(|_| CardSlots::new(d.card_slots).unwrap()),
    }
}

pub(crate) fn settings_of(r: &Rules) -> CheckinSettings {
    CheckinSettings {
        command: r.command.as_str().to_owned(),
        time_zone: r.day.time_zone().to_owned(),
        reset_hour: r.day.reset_hour(),
        card_slots: r.slots.get(),
        reward_id: r.reward.as_ref().map(|w| w.as_str().to_owned()),
    }
}

/// A channel points redemption, as the redemption module announced it through the outbox.
pub(crate) struct Redeemed {
    pub(crate) event_id: String,
    pub(crate) viewer: Viewer,
    pub(crate) reward_id: String,
    /// The Twitch redemption time; a redemption delivered late still counts for the day it happened.
    pub(crate) occurred_at: DateTime<Utc>,
}

/// Redeeming the configured reward checks the viewer in, once per redemption and once per business day. Other
/// rewards, and every reward while none is configured, change nothing. Refunds never undo a check-in.
pub(crate) async fn redeemed<R: CheckinRepository>(
    repo: &R,
    rules: &Rules,
    r: &Redeemed,
    now: DateTime<Utc>,
) -> Result<Option<Outcome>, RepositoryError> {
    if rules
        .reward
        .as_ref()
        .is_none_or(|w| w.as_str() != r.reward_id)
    {
        return Ok(None);
    }
    let check_in = CheckIn::at(&rules.day, r.occurred_at, Trigger::Redemption, now);
    repo.check_in_once(&r.event_id, &r.viewer, &check_in)
        .await
        .map(Some)
}

/// A check-in that was recorded, for the screen events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckedIn {
    pub(crate) viewer_id: String,
    pub(crate) viewer: Viewer,
    pub(crate) total: u64,
    pub(crate) stamps: u32,
    pub(crate) slots: u32,
}

/// Called after every recorded check-in, whatever triggered it.
pub(crate) type OnCheckedIn = Arc<dyn Fn(&CheckedIn) + Send + Sync>;

/// How long a stamp card shows on the overlay; the pause before the next item belongs to the presentation queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    pub show: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            show: Duration::from_secs(5),
        }
    }
}

// Shared with the viewer directory, so they live in `core`.
pub use toolkit_core::twitch::ProfileImages;
pub(crate) use toolkit_core::twitch::is_twitch_image;

const LOOKUP_TIMEOUT: Duration = Duration::from_secs(3);

/// A stamp card for the OBS overlay. `name` is raw viewer text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Card {
    pub(crate) name: String,
    pub(crate) avatar: Option<String>,
    pub(crate) stamps: u32,
    pub(crate) slots: u32,
    pub(crate) total: u64,
    pub(crate) source: Source,
}

impl Card {
    /// This check-in filled the card.
    pub(crate) fn full(&self) -> bool {
        self.stamps == self.slots
    }
}

/// Shows a card for the given time; the infrastructure publishes it to the overlay.
pub(crate) type ShowCard = Arc<dyn Fn(&Card, Duration) + Send + Sync>;

/// Where recorded check-ins go besides the database.
pub(crate) struct Screen {
    pub(crate) changed: OnCheckedIn,
    pub(crate) show_card: ShowCard,
    pub(crate) profiles: Arc<dyn ProfileImages>,
    pub(crate) timing: Timing,
    pub(crate) presenter: Arc<Presenter>,
}

/// The card of a recorded check-in, with the stored profile image or, failing that, one looked up and stored.
async fn card_for<R: CheckinRepository>(
    repo: &R,
    profiles: &dyn ProfileImages,
    c: &CheckedIn,
) -> Card {
    let stored = repo.profile_image(&c.viewer_id).await.ok().flatten();
    // shortcut: a stored image is never refreshed; look it up again when viewers report stale avatars.
    let avatar = match stored.filter(|u| is_twitch_image(u)) {
        Some(url) => Some(url),
        None => {
            let found = tokio::time::timeout(
                LOOKUP_TIMEOUT,
                profiles.profile_image(&c.viewer.twitch_id.0),
            )
            .await
            .ok()
            .flatten()
            .filter(|u| is_twitch_image(u));
            if let Some(url) = &found
                && let Err(e) = repo.set_profile_image(&c.viewer_id, url).await
            {
                tracing::warn!(code = "checkin.storage_failed", error = %e, "profile image not stored");
            }
            found
        }
    };
    Card {
        name: c.viewer.name.clone(),
        avatar,
        stamps: c.stamps,
        slots: c.slots,
        total: c.total,
        source: Source::Twitch,
    }
}

pub(crate) fn checked_in(viewer: &Viewer, outcome: &Outcome, rules: &Rules) -> Option<CheckedIn> {
    match outcome {
        Outcome::CheckedIn { viewer_id, total } => Some(CheckedIn {
            viewer_id: viewer_id.clone(),
            viewer: viewer.clone(),
            total: *total,
            stamps: rules.slots.stamps(*total),
            slots: rules.slots.get(),
        }),
        Outcome::Repeat => None,
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ManualError {
    #[error("checkin.viewer_not_found")]
    ViewerNotFound,
    /// Only today and past business days can be registered.
    #[error("checkin.invalid_date")]
    FutureDay,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

enum Job {
    Chat {
        viewer: Viewer,
        occurred_at: DateTime<Utc>,
    },
    Manual {
        viewer_id: String,
        day: Option<NaiveDate>,
        reply: oneshot::Sender<Result<Outcome, ManualError>>,
    },
}

pub struct CheckinService {
    tx: mpsc::UnboundedSender<Job>,
    presenter: Arc<Presenter>,
    show_card: ShowCard,
    timing: Timing,
    modules: Arc<Modules>,
    config: Arc<Config>,
    on_checked_in: OnCheckedIn,
}

impl CheckinService {
    /// Spawns the worker that records check-ins in order; each recorded one queues its stamp card on the
    /// presentation queue at once, so clearing, stopping and the order cover it while its avatar is looked up.
    /// Needs a Tokio runtime.
    pub(crate) fn start<R: CheckinRepository>(
        repo: R,
        modules: Arc<Modules>,
        config: Arc<Config>,
        screen: Screen,
    ) -> Self {
        let repo = Arc::new(repo);
        let Screen {
            changed,
            show_card,
            profiles,
            timing,
            presenter,
        } = screen;
        let (card_repo, card_presenter, card_show) =
            (repo.clone(), presenter.clone(), show_card.clone());
        let on_checked_in: OnCheckedIn = Arc::new(move |c| {
            changed(c);
            // The card is built (avatar looked up) while it waits its turn; it shows only once built.
            let card: Arc<std::sync::Mutex<Option<Card>>> = Arc::default();
            let (built, repo, profiles, c) =
                (card.clone(), card_repo.clone(), profiles.clone(), c.clone());
            let show_card = card_show.clone();
            card_presenter.push(Item {
                stage: Stage::Checkin,
                subject: Subject::Card {
                    viewer: c.viewer.name.clone(),
                    stamps: c.stamps,
                    slots: c.slots,
                },
                show: timing.show,
                simulated: false,
                ready: Some(Box::pin(async move {
                    *built.lock().unwrap() = Some(card_for(&*repo, &*profiles, &c).await);
                })),
                present: Box::new(move || match card.lock().unwrap().take() {
                    Some(card) => {
                        show_card(&card, timing.show);
                        Shown::Yes
                    }
                    None => Shown::No,
                }),
            });
        });
        let (tx, mut rx) = mpsc::unbounded_channel::<Job>();
        let worker_config = config.clone();
        let notify = on_checked_in.clone();
        tokio::spawn(async move {
            while let Some(job) = rx.recv().await {
                let rules = worker_config.rules();
                match job {
                    Job::Chat {
                        viewer,
                        occurred_at,
                    } => {
                        let check_in =
                            CheckIn::at(&rules.day, occurred_at, Trigger::Chat, Utc::now());
                        match repo.check_in(&viewer, &check_in).await {
                            Ok(outcome) => {
                                if let Some(c) = checked_in(&viewer, &outcome, &rules) {
                                    notify(&c);
                                }
                            }
                            Err(e) => {
                                tracing::error!(code = "checkin.storage_failed", error = %e, "check-in not stored")
                            }
                        }
                    }
                    Job::Manual {
                        viewer_id,
                        day,
                        reply,
                    } => {
                        let result = manual(&*repo, &rules, &viewer_id, day, Utc::now()).await;
                        if let Ok((viewer, outcome)) = &result
                            && let Some(c) = checked_in(viewer, outcome, &rules)
                        {
                            notify(&c);
                        }
                        let _ = reply.send(result.map(|(_, outcome)| outcome));
                    }
                }
            }
        });
        Self {
            tx,
            presenter,
            show_card,
            timing,
            modules,
            config,
            on_checked_in,
        }
    }

    /// Shows a stamp card on the overlay without recording anything (`source=simulation`); `false` when `stamps`
    /// is not between 1 and the card's slots. A card with every slot stamped shows the full-card effect.
    pub fn simulate_card(&self, user_name: &str, stamps: u32) -> bool {
        let slots = self.config.rules().slots.get();
        if !(1..=slots).contains(&stamps) {
            return false;
        }
        let card = Card {
            name: user_name.to_owned(),
            avatar: None,
            stamps,
            slots,
            total: u64::from(stamps),
            source: Source::Simulation,
        };
        let (show_card, show) = (self.show_card.clone(), self.timing.show);
        self.presenter.push(Item {
            stage: Stage::Checkin,
            subject: Subject::Card {
                viewer: card.name.clone(),
                stamps,
                slots,
            },
            show,
            simulated: true,
            ready: None,
            present: Box::new(move || {
                show_card(&card, show);
                Shown::Yes
            }),
        });
        true
    }

    pub(crate) fn config(&self) -> &Arc<Config> {
        &self.config
    }

    pub(crate) fn on_checked_in(&self) -> &OnCheckedIn {
        &self.on_checked_in
    }

    /// A chat message whose first word is the check-in command checks its sender in. Simulated messages never
    /// go into the records; while the module is disabled, messages are dropped like other in-memory events.
    pub fn receive_chat(&self, message: &ChatMessage, source: Source) {
        if source == Source::Simulation || !self.modules.is_enabled(modules::CHECKIN) {
            return;
        }
        if !self
            .config
            .rules
            .read()
            .unwrap()
            .command
            .matches(&message.text)
        {
            return;
        }
        let _ = self.tx.send(Job::Chat {
            viewer: Viewer {
                twitch_id: TwitchUserId(message.user_id.clone()),
                login: message.user_login.clone(),
                name: message.user_name.clone(),
            },
            occurred_at: parse_time(&message.sent_at),
        });
    }

    /// The broadcaster registers a known viewer for today (`day: None`) or back-fills a past business day, under
    /// the same one-per-day rule as the other triggers.
    pub(crate) async fn check_in_manually(
        &self,
        viewer_id: &str,
        day: Option<NaiveDate>,
    ) -> Result<Outcome, ManualError> {
        let (reply, answer) = oneshot::channel();
        let job = Job::Manual {
            viewer_id: viewer_id.to_owned(),
            day,
            reply,
        };
        let stopped = || ManualError::Storage(RepositoryError("check-in worker stopped".into()));
        self.tx.send(job).map_err(|_| stopped())?;
        answer.await.map_err(|_| stopped())?
    }
}

async fn manual<R: CheckinRepository>(
    repo: &R,
    rules: &Rules,
    viewer_id: &str,
    day: Option<NaiveDate>,
    now: DateTime<Utc>,
) -> Result<(Viewer, Outcome), ManualError> {
    let viewer = repo
        .find_viewer(viewer_id)
        .await?
        .ok_or(ManualError::ViewerNotFound)?;
    let check_in = match day {
        None => CheckIn::at(&rules.day, now, Trigger::Manual, now),
        Some(day) if day > rules.day.day_of(now) => return Err(ManualError::FutureDay),
        Some(day) => CheckIn::for_day(&rules.day, day, now),
    };
    let outcome = repo.check_in(&viewer, &check_in).await?;
    Ok((viewer, outcome))
}

/// How an import changes the viewers: new ones, and known ones whose baseline is replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ImportCounts {
    pub(crate) added: u64,
    pub(crate) updated: u64,
}

/// Counts what importing `viewers` changes and, when `apply`, writes it. The baseline replaces the previous one,
/// so the same file imported twice gives the same totals.
pub(crate) async fn import<R: CheckinRepository>(
    repo: &R,
    viewers: &[ImportedViewer],
    apply: bool,
    now: DateTime<Utc>,
) -> Result<ImportCounts, RepositoryError> {
    let known = repo.known_viewers().await?;
    let updated = viewers
        .iter()
        .filter(|v| known.contains(&v.twitch_id.0))
        .count() as u64;
    if apply {
        repo.import(viewers, now).await?;
    }
    Ok(ImportCounts {
        added: viewers.len() as u64 - updated,
        updated,
    })
}

/// The time the event happened; an unreadable one falls back to the time received.
pub(crate) fn parse_time(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .map(|t| t.to_utc())
        .unwrap_or_else(|_| {
            tracing::warn!("unreadable event time; using the time received");
            Utc::now()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unusable_saved_values_fall_back_one_by_one() {
        let saved = CheckinSettings {
            command: "two words".into(),
            time_zone: "Europe/Berlin".into(),
            reset_hour: 30,
            card_slots: 12,
            reward_id: Some("not valid!".into()),
        };
        let r = rules_or_defaults(&saved);
        let s = settings_of(&r);
        assert_eq!(s.command, "打卡");
        assert_eq!((s.time_zone.as_str(), s.reset_hour), ("Europe/Berlin", 0));
        assert_eq!(s.card_slots, 12);
        assert_eq!(s.reward_id, None);
        assert!(
            rules(&saved).is_err(),
            "an update with the same values is refused"
        );
    }
}
