//! Check-in use cases: a chat command checks a viewer in, once per business day. Check-ins are handled one at a
//! time, in arrival order; the repository also refuses a repeat on its own.

use std::{
    future::Future,
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};

use tokio::sync::{mpsc, oneshot};
use toolkit_core::time::Timestamp;
use toolkit_core::{
    commands::{Allowed, CommandEntry, CommandError, CommandTable},
    events::{ChatMessage, Source},
    modules::{self, Modules},
    presentation::{
        Item, PlayResult, PlayedKind, PlayedReporter, Presenter, Recent, RecentItem, Shown, Stage,
        Subject, reporting,
    },
    settings::{self, CheckinSettings},
};

mod style;

pub(crate) use style::{IMAGE_MAX_BYTES, StyleError};

use crate::domain::{
    BusinessDay, CardSlots, CheckIn, Command, Deduction, ImportedViewer, InvalidSettings, RewardId,
    Rules, Trigger, TwitchUserId, Viewer,
};

#[derive(Debug, thiserror::Error)]
#[error("checkin.storage_failed: {0}")]
pub(crate) struct RepositoryError(pub(crate) String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Recorded; the record's ID, the viewer's internal ID and total check-ins after it.
    CheckedIn {
        record_id: String,
        viewer_id: String,
        total: u64,
    },
    /// The viewer already checked in on that business day; nothing was written.
    Repeat,
    /// An outbox event handled before (redelivered); nothing was written and nothing shows.
    Redelivered,
}

/// What became of a deduction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Deducted {
    /// Recorded; the stamps still available after it.
    Done {
        available: u64,
    },
    /// More than is available; nothing was written.
    NotEnough {
        available: u64,
    },
    UnknownViewer,
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
        now: Timestamp,
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

    /// A viewer's internal ID by Twitch user ID.
    fn viewer_id(
        &self,
        twitch_id: &TwitchUserId,
    ) -> impl Future<Output = Result<Option<String>, RepositoryError>> + Send;

    /// The viewer's lifetime check-ins; `None` for an unknown viewer.
    fn total(
        &self,
        viewer_id: &str,
    ) -> impl Future<Output = Result<Option<u64>, RepositoryError>> + Send;

    /// In one transaction: works out what the viewer has available and records `deduction` only if it fits.
    fn deduct(
        &self,
        viewer_id: &str,
        deduction: &Deduction,
        now: Timestamp,
    ) -> impl Future<Output = Result<Deducted, RepositoryError>> + Send;
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ConfigError {
    #[error(transparent)]
    Invalid(#[from] InvalidSettings),
    #[error(transparent)]
    Command(#[from] CommandError),
}

/// The check-in command's key in the command table.
pub const COMMAND_KEY: &str = "checkin";

/// A command table with only this module's command, for tests.
#[cfg(test)]
pub(crate) fn test_commands(dir: &std::path::Path) -> Arc<CommandTable> {
    Arc::new(CommandTable::load(
        dir.to_path_buf(),
        crate::COMMANDS.to_vec(),
    ))
}

/// The check-in settings in force, kept in `settings.json` (the command in the shared command table) and in
/// memory so chat never waits on the file.
pub(crate) struct Config {
    commands: Arc<CommandTable>,
    rules: RwLock<Rules>,
    data_dir: PathBuf,
    /// The stamp card's look, checked.
    style: RwLock<toolkit_core::settings::CardStyle>,
}

impl Config {
    /// Values in `settings.json` that cannot be used fall back to their defaults one by one. A command saved by
    /// an older version (in the check-in settings) moves to the command table first.
    pub(crate) fn load(data_dir: PathBuf, commands: Arc<CommandTable>) -> Self {
        let saved = settings::load(&data_dir).checkin;
        if commands.saved(COMMAND_KEY).is_none()
            && let Ok(command) = Command::new(&saved.command)
        {
            let entry = CommandEntry {
                name: command.as_str().to_owned(),
                allowed: Allowed::Everyone,
                module: modules::CHECKIN.into(),
                label: None,
            };
            if let Err(e) = commands.set(COMMAND_KEY, entry, |_| {}) {
                // It keeps working with the old command; the move is tried again at the next start.
                tracing::warn!(code = %e, "check-in command not moved to the command table");
            }
        }
        let rules = rules_or_defaults(&saved, &commands.get(COMMAND_KEY));
        let style = crate::domain::style::checked(saved.card).unwrap_or_default();
        Self {
            commands,
            rules: RwLock::new(rules),
            data_dir,
            style: RwLock::new(style),
        }
    }

    pub(crate) fn rules(&self) -> Rules {
        self.rules.read().unwrap().clone()
    }

    /// Validates, saves, then applies; nothing changes when either step fails, or when another feature's command
    /// has the name.
    pub(crate) fn update(
        &self,
        next: &CheckinSettings,
        allowed: Allowed,
    ) -> Result<Rules, ConfigError> {
        let rules = rules(next, allowed)?;
        let mut current = self.rules.write().unwrap();
        let entry = CommandEntry {
            name: rules.command.as_str().to_owned(),
            allowed,
            module: modules::CHECKIN.into(),
            label: None,
        };
        // The card style is saved on its own; keep it.
        self.commands.set(COMMAND_KEY, entry, |s| {
            let card = std::mem::take(&mut s.checkin.card);
            s.checkin = settings_of(&rules);
            s.checkin.card = card;
        })?;
        *current = rules.clone();
        Ok(rules)
    }
}

pub(crate) fn rules(s: &CheckinSettings, allowed: Allowed) -> Result<Rules, InvalidSettings> {
    Ok(Rules {
        command: Command::new(&s.command)?,
        allowed,
        reward: s.reward_id.as_deref().map(RewardId::new).transpose()?,
        day: BusinessDay::new(&s.time_zone, s.reset_hour)?,
        slots: CardSlots::new(s.card_slots)?,
    })
}

fn rules_or_defaults(s: &CheckinSettings, command: &CommandEntry) -> Rules {
    let d = CheckinSettings::default();
    Rules {
        command: Command::new(&command.name).unwrap_or_else(|_| Command::new(&d.command).unwrap()),
        allowed: command.allowed,
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
        card: Default::default(),
    }
}

/// A channel points redemption, as the redemption module announced it through the outbox.
pub(crate) struct Redeemed {
    pub(crate) event_id: String,
    pub(crate) viewer: Viewer,
    pub(crate) reward_id: String,
    /// The Twitch redemption time; a redemption delivered late still counts for the day it happened.
    pub(crate) occurred_at: Timestamp,
}

/// Redeeming the configured reward checks the viewer in, once per redemption and once per business day. Other
/// rewards, and every reward while none is configured, change nothing. Refunds never undo a check-in.
pub(crate) async fn redeemed<R: CheckinRepository>(
    repo: &R,
    rules: &Rules,
    r: &Redeemed,
    now: Timestamp,
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
    /// The check-in record's ID.
    pub(crate) record_id: String,
    pub(crate) viewer_id: String,
    pub(crate) viewer: Viewer,
    pub(crate) total: u64,
    pub(crate) stamps: u32,
    pub(crate) slots: u32,
    /// The Twitch redemption that checked the viewer in, if one did.
    pub(crate) redemption: Option<String>,
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
/// Takes the card on the overlay away at once.
pub(crate) type HideCard = Arc<dyn Fn() + Send + Sync>;
/// Queues a card on the presentation queue; `true` records what became of it for the event feed.
type QueueCard = Arc<dyn Fn(&CheckedIn, bool) + Send + Sync>;

/// Where recorded check-ins go besides the database.
pub(crate) struct Screen {
    pub(crate) changed: OnCheckedIn,
    pub(crate) show_card: ShowCard,
    pub(crate) hide_card: HideCard,
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
        Outcome::CheckedIn {
            record_id,
            viewer_id,
            total,
        } => Some(CheckedIn {
            record_id: record_id.clone(),
            viewer_id: viewer_id.clone(),
            viewer: viewer.clone(),
            total: *total,
            stamps: rules.slots.stamps(*total),
            slots: rules.slots.get(),
            redemption: None,
        }),
        Outcome::Repeat | Outcome::Redelivered => None,
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
        occurred_at: Timestamp,
    },
    Manual {
        viewer_id: String,
        day: Option<jiff::civil::Date>,
        reply: oneshot::Sender<Result<Outcome, ManualError>>,
    },
    Deduct {
        viewer_id: String,
        deduction: Deduction,
        reply: oneshot::Sender<Result<Deducted, RepositoryError>>,
    },
    /// Queues a viewer's card without recording anything; `false` for an unknown viewer.
    Push {
        viewer_id: String,
        reply: oneshot::Sender<Result<bool, RepositoryError>>,
    },
}

pub struct CheckinService {
    tx: mpsc::UnboundedSender<Job>,
    /// What became of the latest stamp cards.
    recent: Recent,
    presenter: Arc<Presenter>,
    show_card: ShowCard,
    hide_card: HideCard,
    timing: Timing,
    modules: Arc<Modules>,
    config: Arc<Config>,
    on_checked_in: OnCheckedIn,
    queue_card: QueueCard,
    /// Tells the redemption module what each redemption's card did.
    played: PlayedReporter,
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
            hide_card,
            profiles,
            timing,
            presenter,
        } = screen;
        let recent = Recent::default();
        let played = PlayedReporter::default();
        let (card_repo, card_presenter, card_show, card_recent, card_played) = (
            repo.clone(),
            presenter.clone(),
            show_card.clone(),
            recent.clone(),
            played.clone(),
        );
        let queue_card: QueueCard = Arc::new(move |c, record| {
            // The card is built (avatar looked up) while it waits its turn; it shows only once built.
            let card: Arc<std::sync::Mutex<Option<Card>>> = Arc::default();
            let (built, repo, profiles, c) =
                (card.clone(), card_repo.clone(), profiles.clone(), c.clone());
            let show_card = card_show.clone();
            let subject = Subject::Card {
                viewer: c.viewer.name.clone(),
                stamps: c.stamps,
                slots: c.slots,
            };
            // A pushed card is no check-in: the event feed never lists it.
            let done = record.then(|| {
                let done =
                    card_recent.record(Some(c.record_id.clone()), subject.clone(), Source::Twitch);
                match c.redemption.clone() {
                    Some(id) => reporting(
                        done,
                        card_played.clone(),
                        id,
                        PlayedKind::Card,
                        PlayResult::from,
                    ),
                    None => done,
                }
            });
            card_presenter.push(Item {
                stage: Stage::Checkin,
                subject,
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
                done,
            });
        });
        let (queued, repeat_card, kept) =
            (queue_card.clone(), queue_card.clone(), queue_card.clone());
        let on_checked_in: OnCheckedIn = Arc::new(move |c| {
            changed(c);
            queued(c, true);
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
                            CheckIn::at(&rules.day, occurred_at, Trigger::Chat, Timestamp::now());
                        match repo.check_in(&viewer, &check_in).await {
                            Ok(Outcome::Repeat) if worker_config.style().show_repeat => {
                                if let Ok(Some(c)) = repeated(&*repo, &rules, &viewer).await {
                                    repeat_card(&c, false);
                                }
                            }
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
                        let result =
                            manual(&*repo, &rules, &viewer_id, day, Timestamp::now()).await;
                        if let Ok((viewer, outcome)) = &result
                            && let Some(c) = checked_in(viewer, outcome, &rules)
                        {
                            notify(&c);
                        }
                        let _ = reply.send(result.map(|(_, outcome)| outcome));
                    }
                    Job::Deduct {
                        viewer_id,
                        deduction,
                        reply,
                    } => {
                        let _ =
                            reply.send(repo.deduct(&viewer_id, &deduction, Timestamp::now()).await);
                    }
                    Job::Push { viewer_id, reply } => {
                        let found = pushed(&*repo, &rules, &viewer_id).await;
                        if let Ok(Some(c)) = &found {
                            queue_card(c, false);
                        }
                        let _ = reply.send(found.map(|c| c.is_some()));
                    }
                }
            }
        });
        Self {
            tx,
            recent,
            presenter,
            show_card,
            hide_card,
            timing,
            modules,
            config,
            on_checked_in,
            queue_card: kept,
            played,
        }
    }

    /// Where it reports what each redemption's card did; the host connects it to the outbox.
    pub fn played(&self) -> &PlayedReporter {
        &self.played
    }

    /// Shows a repeat check-in's card when the style asks for it; nothing is recorded.
    pub(crate) fn repeat_card(&self) -> OnCheckedIn {
        let queue = self.queue_card.clone();
        Arc::new(move |c| queue(c, false))
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
        let subject = Subject::Card {
            viewer: card.name.clone(),
            stamps,
            slots,
        };
        let done = self
            .recent
            .record(None, subject.clone(), Source::Simulation);
        self.presenter.push(Item {
            stage: Stage::Checkin,
            subject,
            show,
            simulated: true,
            ready: None,
            present: Box::new(move || {
                show_card(&card, show);
                Shown::Yes
            }),
            done: Some(done),
        });
        true
    }

    /// The latest stamp cards queued (this launch only, simulated ones too), newest first, with what became of
    /// each.
    pub fn recent(&self) -> Vec<RecentItem> {
        self.recent.list()
    }

    pub(crate) fn config(&self) -> &Arc<Config> {
        &self.config
    }

    /// Points check-ins at reward `to` when they use `from` (a reward taken over); `true` when it changed. The error
    /// is the reason the settings were not saved.
    pub fn repoint_reward(&self, from: &str, to: &str) -> Result<bool, String> {
        let rules = self.config.rules();
        if rules.reward.as_ref().is_none_or(|r| r.as_str() != from) {
            return Ok(false);
        }
        let mut next = settings_of(&rules);
        next.reward_id = Some(to.to_owned());
        self.config
            .update(&next, rules.allowed)
            .map(|_| true)
            .map_err(|e| e.to_string())
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
        let rules = self.config.rules.read().unwrap();
        if !rules.command.matches(&message.text) || !rules.allowed.permits(message) {
            return;
        }
        drop(rules);
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
        day: Option<jiff::civil::Date>,
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

    /// Puts a viewer's card on the screen through the presentation queue, without recording a check-in; `false` for
    /// an unknown viewer.
    pub(crate) async fn push_card(&self, viewer_id: &str) -> Result<bool, RepositoryError> {
        let (reply, answer) = oneshot::channel();
        let job = Job::Push {
            viewer_id: viewer_id.to_owned(),
            reply,
        };
        let stopped = || RepositoryError("check-in worker stopped".into());
        self.tx.send(job).map_err(|_| stopped())?;
        answer.await.map_err(|_| stopped())?
    }

    /// When this month started, by the business day: its first day's start in the check-in time zone and reset hour.
    pub fn month_start(&self, now: Timestamp) -> Timestamp {
        let rules = self.config.rules();
        let today = rules.day.day_of(now);
        let first = today.first_of_month();
        rules.day.window(first).0
    }

    /// Takes the card on the screen away at once.
    pub(crate) fn hide_card(&self) {
        (self.hide_card)();
    }

    /// Spends a viewer's stamps; handled in turn with the check-ins, so two at once never go below 0.
    pub(crate) async fn deduct(
        &self,
        viewer_id: &str,
        deduction: Deduction,
    ) -> Result<Deducted, RepositoryError> {
        let (reply, answer) = oneshot::channel();
        let job = Job::Deduct {
            viewer_id: viewer_id.to_owned(),
            deduction,
            reply,
        };
        let stopped = || RepositoryError("check-in worker stopped".into());
        self.tx.send(job).map_err(|_| stopped())?;
        answer.await.map_err(|_| stopped())?
    }
}

/// The card a repeat check-in shows: the viewer's card as it stands, under no record.
pub(crate) async fn repeated<R: CheckinRepository>(
    repo: &R,
    rules: &Rules,
    viewer: &Viewer,
) -> Result<Option<CheckedIn>, RepositoryError> {
    match repo.viewer_id(&viewer.twitch_id).await? {
        Some(id) => pushed(repo, rules, &id).await,
        None => Ok(None),
    }
}

/// The card a push shows: the viewer's card as of their total, under no record.
async fn pushed<R: CheckinRepository>(
    repo: &R,
    rules: &Rules,
    viewer_id: &str,
) -> Result<Option<CheckedIn>, RepositoryError> {
    let (Some(viewer), Some(total)) = (
        repo.find_viewer(viewer_id).await?,
        repo.total(viewer_id).await?,
    ) else {
        return Ok(None);
    };
    Ok(Some(CheckedIn {
        record_id: String::new(),
        viewer_id: viewer_id.to_owned(),
        viewer,
        total,
        stamps: rules.slots.stamps(total),
        slots: rules.slots.get(),
        redemption: None,
    }))
}

async fn manual<R: CheckinRepository>(
    repo: &R,
    rules: &Rules,
    viewer_id: &str,
    day: Option<jiff::civil::Date>,
    now: Timestamp,
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
    now: Timestamp,
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
pub(crate) fn parse_time(text: &str) -> Timestamp {
    text.parse::<Timestamp>().unwrap_or_else(|_| {
        tracing::warn!("unreadable event time; using the time received");
        Timestamp::now()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_saved_by_an_older_version_moves_to_the_command_table() {
        let dir = std::env::temp_dir().join(format!("st-checkin-move-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"checkin":{"command":"!簽到","card_slots":12}}"#,
        )
        .unwrap();
        let config = Config::load(dir.clone(), test_commands(&dir));
        assert_eq!(config.rules().command.as_str(), "簽到");
        let saved = settings::load(&dir);
        assert_eq!(saved.chat_commands[COMMAND_KEY].name, "簽到");
        assert_eq!(saved.checkin.card_slots, 12);
        // The old place is no longer written.
        let text = std::fs::read_to_string(dir.join("settings.json")).unwrap();
        assert!(!text.contains("\"command\""), "{text}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unusable_saved_values_fall_back_one_by_one() {
        let saved = CheckinSettings {
            command: "two words".into(),
            time_zone: "Europe/Berlin".into(),
            reset_hour: 30,
            card_slots: 12,
            reward_id: Some("not valid!".into()),
            card: Default::default(),
        };
        let bad = CommandEntry {
            name: "two words".into(),
            allowed: Allowed::Vip,
            module: modules::CHECKIN.into(),
            label: None,
        };
        let r = rules_or_defaults(&saved, &bad);
        let s = settings_of(&r);
        assert_eq!((s.command.as_str(), r.allowed), ("打卡", Allowed::Vip));
        assert_eq!((s.time_zone.as_str(), s.reset_hour), ("Europe/Berlin", 0));
        assert_eq!(s.card_slots, 12);
        assert_eq!(s.reward_id, None);
        assert!(
            rules(&saved, Allowed::Everyone).is_err(),
            "an update with the same values is refused"
        );
    }
}
