//! Check-in use cases: a chat command checks a viewer in, once per business day. Check-ins are handled one at a
//! time, in arrival order; the repository also refuses a repeat on its own.

use std::{
    future::Future,
    path::PathBuf,
    sync::{Arc, RwLock},
};

use chrono::{DateTime, Utc};
use tokio::sync::mpsc;
use toolkit_core::{
    events::{ChatMessage, Source},
    modules::{self, Modules},
    settings::{self, CheckinSettings},
};

use crate::domain::{
    BusinessDay, CardSlots, CheckIn, Command, InvalidSettings, RewardId, Rules, Trigger,
    TwitchUserId, Viewer,
};

#[derive(Debug, thiserror::Error)]
#[error("checkin.storage_failed: {0}")]
pub(crate) struct RepositoryError(pub(crate) String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Recorded; the viewer's total check-ins after it.
    CheckedIn { total: u64 },
    /// The viewer already checked in on that business day; nothing was written.
    Repeat,
}

/// Storage port; the SQLite implementation lives in `infrastructure::db`.
pub(crate) trait CheckinRepository: Send + Sync + 'static {
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

struct Job {
    viewer: Viewer,
    occurred_at: DateTime<Utc>,
    trigger: Trigger,
}

pub struct CheckinService {
    tx: mpsc::UnboundedSender<Job>,
    modules: Arc<Modules>,
    config: Arc<Config>,
}

impl CheckinService {
    /// Spawns the worker that records check-ins in order; needs a Tokio runtime.
    pub(crate) fn start<R: CheckinRepository>(
        repo: R,
        modules: Arc<Modules>,
        config: Arc<Config>,
    ) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<Job>();
        let worker_config = config.clone();
        tokio::spawn(async move {
            while let Some(job) = rx.recv().await {
                let rules = worker_config.rules();
                let check_in = CheckIn::at(&rules.day, job.occurred_at, job.trigger, Utc::now());
                if let Err(e) = repo.check_in(&job.viewer, &check_in).await {
                    tracing::error!(code = "checkin.storage_failed", error = %e, "check-in not stored");
                }
            }
        });
        Self {
            tx,
            modules,
            config,
        }
    }

    pub(crate) fn config(&self) -> &Arc<Config> {
        &self.config
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
        let _ = self.tx.send(Job {
            viewer: Viewer {
                twitch_id: TwitchUserId(message.user_id.clone()),
                login: message.user_login.clone(),
                name: message.user_name.clone(),
            },
            occurred_at: parse_time(&message.sent_at),
            trigger: Trigger::Chat,
        });
    }
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
