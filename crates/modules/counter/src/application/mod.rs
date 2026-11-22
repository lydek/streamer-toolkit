//! Counter use cases: the list, chat commands that count, buttons, and the per-stream reset. Every change is
//! announced with all values.

use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, RwLock},
};

use chrono::{DateTime, Utc};

use toolkit_core::{
    commands::{Allowed, CommandEntry, CommandError, CommandTable},
    events::{Alert, ChatMessage, Source},
    modules::{self, Modules},
};
use uuid::Uuid;

use crate::domain::{
    self, Counter, ViewerCount, ViewerCounter,
    marathon::{Clock, Marathon, MarathonOptions},
};

/// Alert IDs remembered so a redelivered alert adds no time twice.
const SEEN_ALERTS: usize = 500;

#[derive(Debug, thiserror::Error)]
#[error("counter.storage_failed: {0}")]
pub struct RepositoryError(pub(crate) String);

#[derive(Debug, thiserror::Error)]
pub enum CounterError {
    /// An empty name, a command that is not one word, or too many counters.
    #[error("counter.invalid")]
    Invalid,
    #[error("counter.not_found")]
    NotFound,
    /// A viewer the viewer directory does not have.
    #[error("counter.viewer_not_found")]
    ViewerNotFound,
    #[error(transparent)]
    Command(#[from] CommandError),
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

pub trait CounterRepository: Send + Sync + 'static {
    /// In creation order.
    fn list(&self) -> impl Future<Output = Result<Vec<Counter>, RepositoryError>> + Send;
    fn insert(&self, counter: &Counter)
    -> impl Future<Output = Result<(), RepositoryError>> + Send;
    /// Name, per-stream and reset session; the value stays as it is. `false` when there is no such counter.
    fn update(
        &self,
        counter: &Counter,
    ) -> impl Future<Output = Result<bool, RepositoryError>> + Send;
    fn delete(&self, id: &str) -> impl Future<Output = Result<bool, RepositoryError>> + Send;
    /// Adds `delta`, never going below 0; `false` when there is no such counter.
    fn add(
        &self,
        id: &str,
        delta: i64,
    ) -> impl Future<Output = Result<bool, RepositoryError>> + Send;
    fn reset(&self, id: &str) -> impl Future<Output = Result<bool, RepositoryError>> + Send;
    /// Puts every per-stream counter not yet reset for `session` back to 0; returns how many.
    fn reset_for_session(
        &self,
        session: &str,
    ) -> impl Future<Output = Result<u64, RepositoryError>> + Send;

    /// Per-viewer counters, in creation order.
    fn viewer_counters(
        &self,
    ) -> impl Future<Output = Result<Vec<ViewerCounter>, RepositoryError>> + Send;
    fn insert_viewer_counter(
        &self,
        counter: &ViewerCounter,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
    fn rename_viewer_counter(
        &self,
        id: &str,
        name: &str,
    ) -> impl Future<Output = Result<bool, RepositoryError>> + Send;
    /// The counter and every viewer's count in it.
    fn delete_viewer_counter(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<bool, RepositoryError>> + Send;
    /// Adds `delta` to the viewer's count, never going below 0; `false` when there is no such counter.
    fn add_for_viewer(
        &self,
        counter_id: &str,
        twitch_user_id: &str,
        delta: i64,
    ) -> impl Future<Output = Result<bool, RepositoryError>> + Send;
    /// The viewers above 0, most first (ties by Twitch user ID), at most `limit`, named from the viewer directory.
    fn viewer_counts(
        &self,
        counter_id: &str,
        limit: Option<u32>,
    ) -> impl Future<Output = Result<Vec<ViewerCount>, RepositoryError>> + Send;
    /// Every viewer's count added up.
    fn viewer_total(
        &self,
        counter_id: &str,
    ) -> impl Future<Output = Result<i64, RepositoryError>> + Send;
    /// Viewers in the directory whose login or display name contains `query`, with a value of 0.
    fn search_viewers(
        &self,
        query: &str,
    ) -> impl Future<Output = Result<Vec<ViewerCount>, RepositoryError>> + Send;
    /// Whether the viewer directory has the viewer.
    fn viewer_known(
        &self,
        twitch_user_id: &str,
    ) -> impl Future<Output = Result<bool, RepositoryError>> + Send;

    /// The sub marathon timer as last saved; `None` before it was ever saved.
    fn load_marathon(
        &self,
    ) -> impl Future<Output = Result<Option<Marathon>, RepositoryError>> + Send;
    fn save_marathon(
        &self,
        marathon: &Marathon,
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
}

/// The sub marathon timer as the overlay and the admin UI show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarathonStatus {
    /// Its remaining time is as of its anchor, which is the time of this status.
    pub marathon: Marathon,
    /// Counting down now: live, not paused and time left.
    pub running: bool,
    /// The toolkit has seen the stream live.
    pub live: bool,
}

/// A stream status, for the per-stream reset and the marathon clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    Live { started_at: DateTime<Utc> },
    Offline,
}

/// Announces the marathon timer after every change.
pub type AnnounceMarathon = Box<dyn Fn(&MarathonStatus) + Send + Sync>;

/// A per-viewer counter with its total and its top three.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerCounterSummary {
    pub counter: ViewerCounter,
    pub total: i64,
    pub top: Vec<ViewerCount>,
}

/// The stream session now (the live one, or the last one after it ended; `None` before any).
pub type CurrentSession =
    Box<dyn Fn() -> Pin<Box<dyn Future<Output = Option<String>> + Send>> + Send + Sync>;
/// Announces every counter's value after a change.
pub type Announce = Box<dyn Fn(&[Counter]) + Send + Sync>;

/// What the admin UI sets on a counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterSettings {
    pub name: String,
    /// Without its `!`; empty: no chat command.
    pub command: String,
    pub allowed: Allowed,
    pub per_stream: bool,
}

pub struct CounterService<R> {
    repo: R,
    commands: Arc<CommandTable>,
    modules: Arc<Modules>,
    current_session: CurrentSession,
    announce: Announce,
    announce_marathon: AnnounceMarathon,
    /// The counters' IDs, so chat never waits on the database to find a command.
    ids: RwLock<Vec<String>>,
    /// Loaded on first use; one change at a time, each saved before the next.
    clock: tokio::sync::Mutex<Option<Clock>>,
    seen_alerts: Mutex<VecDeque<String>>,
}

impl<R: CounterRepository> CounterService<R> {
    /// Needs a Tokio runtime: the counters load in the background (chat commands count once they have).
    pub(crate) fn start(
        repo: R,
        commands: Arc<CommandTable>,
        modules: Arc<Modules>,
        current_session: CurrentSession,
        announce: Announce,
        announce_marathon: AnnounceMarathon,
    ) -> Arc<Self> {
        let service = Arc::new(Self {
            repo,
            commands,
            modules,
            current_session,
            announce,
            announce_marathon,
            ids: RwLock::new(Vec::new()),
            clock: tokio::sync::Mutex::new(None),
            seen_alerts: Mutex::new(VecDeque::new()),
        });
        let loading = service.clone();
        tokio::spawn(async move {
            match loading.repo.list().await {
                Ok(all) => {
                    *loading.ids.write().unwrap() = all.iter().map(|c| c.id.clone()).collect();
                    (loading.announce)(&all);
                }
                Err(e) => {
                    tracing::error!(code = "counter.storage_failed", error = %e.0, "counters not loaded")
                }
            }
            if let Err(e) = loading.marathon().await {
                tracing::error!(code = "counter.storage_failed", error = %e.0, "marathon timer not loaded");
            }
        });
        service
    }

    pub async fn list(&self) -> Result<Vec<Counter>, RepositoryError> {
        self.repo.list().await
    }

    /// The command and who may use it for a counter.
    pub fn command(&self, id: &str) -> CommandEntry {
        self.commands.get(&domain::command_key(id))
    }

    async fn announce_all(&self) {
        match self.repo.list().await {
            Ok(all) => (self.announce)(&all),
            Err(e) => {
                tracing::error!(code = "counter.storage_failed", error = %e.0, "counters not announced")
            }
        }
    }

    pub async fn create(&self, settings: CounterSettings) -> Result<Counter, CounterError> {
        if self.ids.read().unwrap().len() >= domain::MAX_COUNTERS {
            return Err(CounterError::Invalid);
        }
        let mut counter = Counter {
            id: Uuid::now_v7().to_string(),
            name: String::new(),
            value: 0,
            per_stream: false,
            reset_session: None,
        };
        self.apply(&mut counter, settings).await?;
        self.repo.insert(&counter).await?;
        self.ids.write().unwrap().push(counter.id.clone());
        self.announce_all().await;
        Ok(counter)
    }

    pub async fn update(
        &self,
        id: &str,
        settings: CounterSettings,
    ) -> Result<Counter, CounterError> {
        let mut counter = self
            .repo
            .list()
            .await?
            .into_iter()
            .find(|c| c.id == id)
            .ok_or(CounterError::NotFound)?;
        self.apply(&mut counter, settings).await?;
        if !self.repo.update(&counter).await? {
            return Err(CounterError::NotFound);
        }
        self.announce_all().await;
        Ok(counter)
    }

    /// Checks `settings`, saves its command, and fills the counter in. Becoming per-stream takes the current
    /// session as already reset, so nothing resets at once, even mid-stream.
    async fn apply(
        &self,
        counter: &mut Counter,
        settings: CounterSettings,
    ) -> Result<(), CounterError> {
        let name = domain::clean_name(&settings.name).ok_or(CounterError::Invalid)?;
        let command = domain::command_name(&settings.command).ok_or(CounterError::Invalid)?;
        if settings.per_stream && !counter.per_stream {
            counter.reset_session = (self.current_session)().await;
        }
        let key = domain::command_key(&counter.id);
        if command.is_empty() {
            self.commands.remove(&key)?;
        } else {
            let entry = CommandEntry {
                name: command,
                allowed: settings.allowed,
                module: modules::COUNTER.into(),
                label: Some(name.clone()),
            };
            self.commands.set(&key, entry, |_| {})?;
        }
        counter.name = name;
        counter.per_stream = settings.per_stream;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<(), CounterError> {
        if !self.repo.delete(id).await? {
            return Err(CounterError::NotFound);
        }
        self.ids.write().unwrap().retain(|i| i != id);
        // Its command goes too; a failure only leaves the name taken until it is saved again.
        if let Err(e) = self.commands.remove(&domain::command_key(id)) {
            tracing::warn!(code = %e, "a deleted counter's command was left in the table");
        }
        self.announce_all().await;
        Ok(())
    }

    /// Adds `delta` (never below 0).
    pub async fn add(&self, id: &str, delta: i64) -> Result<(), CounterError> {
        if !self.repo.add(id, delta).await? {
            return Err(CounterError::NotFound);
        }
        self.announce_all().await;
        Ok(())
    }

    pub async fn reset(&self, id: &str) -> Result<(), CounterError> {
        if !self.repo.reset(id).await? {
            return Err(CounterError::NotFound);
        }
        self.announce_all().await;
        Ok(())
    }

    /// The counters whose command `message` starts with, if its sender may use it. Simulated messages never count;
    /// while the module is disabled, commands are dropped like other in-memory events.
    pub fn counted_by(&self, message: &ChatMessage, source: Source) -> Vec<String> {
        if source == Source::Simulation || !self.modules.is_enabled(modules::COUNTER) {
            return Vec::new();
        }
        self.ids
            .read()
            .unwrap()
            .iter()
            .filter(|id| {
                let command = self.command(id);
                command.matches(&message.text) && command.allowed.permits(message)
            })
            .cloned()
            .collect()
    }

    /// A chat message adds one to every counter whose command it starts with (see [`Self::counted_by`]).
    pub fn receive_chat(self: &Arc<Self>, message: &ChatMessage, source: Source) {
        let ids = self.counted_by(message, source);
        if ids.is_empty() {
            return;
        }
        let service = self.clone();
        tokio::spawn(async move {
            for id in ids {
                if let Err(e) = service.repo.add(&id, 1).await {
                    tracing::error!(code = "counter.storage_failed", error = %e.0, "chat count not stored");
                }
            }
            service.announce_all().await;
        });
    }

    pub async fn viewer_counters(&self) -> Result<Vec<ViewerCounterSummary>, RepositoryError> {
        let mut all = Vec::new();
        for counter in self.repo.viewer_counters().await? {
            let total = self.repo.viewer_total(&counter.id).await?;
            let top = self.repo.viewer_counts(&counter.id, Some(3)).await?;
            all.push(ViewerCounterSummary {
                counter,
                total,
                top,
            });
        }
        Ok(all)
    }

    pub async fn create_viewer_counter(&self, name: &str) -> Result<ViewerCounter, CounterError> {
        let name = domain::clean_name(name).ok_or(CounterError::Invalid)?;
        if self.repo.viewer_counters().await?.len() >= domain::MAX_COUNTERS {
            return Err(CounterError::Invalid);
        }
        let counter = ViewerCounter {
            id: Uuid::now_v7().to_string(),
            name,
        };
        self.repo.insert_viewer_counter(&counter).await?;
        Ok(counter)
    }

    pub async fn rename_viewer_counter(
        &self,
        id: &str,
        name: &str,
    ) -> Result<ViewerCounter, CounterError> {
        let name = domain::clean_name(name).ok_or(CounterError::Invalid)?;
        match self.repo.rename_viewer_counter(id, &name).await? {
            true => Ok(ViewerCounter {
                id: id.into(),
                name,
            }),
            false => Err(CounterError::NotFound),
        }
    }

    pub async fn delete_viewer_counter(&self, id: &str) -> Result<(), CounterError> {
        match self.repo.delete_viewer_counter(id).await? {
            true => Ok(()),
            false => Err(CounterError::NotFound),
        }
    }

    /// Adds or takes one for a viewer from the viewer directory (never below 0).
    pub async fn add_for_viewer(
        &self,
        id: &str,
        twitch_user_id: &str,
        delta: i64,
    ) -> Result<(), CounterError> {
        if !self.repo.viewer_known(twitch_user_id).await? {
            return Err(CounterError::ViewerNotFound);
        }
        match self.repo.add_for_viewer(id, twitch_user_id, delta).await? {
            true => Ok(()),
            false => Err(CounterError::NotFound),
        }
    }

    /// Every viewer above 0 in a per-viewer counter, most first.
    pub async fn viewer_counts(&self, id: &str) -> Result<Vec<ViewerCount>, CounterError> {
        if !self
            .repo
            .viewer_counters()
            .await?
            .iter()
            .any(|c| c.id == id)
        {
            return Err(CounterError::NotFound);
        }
        Ok(self.repo.viewer_counts(id, None).await?)
    }

    /// Viewers to count for, with their count in `id` (0 when they have none).
    pub async fn search_viewers(
        &self,
        id: &str,
        query: &str,
    ) -> Result<Vec<ViewerCount>, CounterError> {
        let counts = self.viewer_counts(id).await?;
        let mut found = self.repo.search_viewers(query).await?;
        for v in &mut found {
            v.value = counts
                .iter()
                .find(|c| c.twitch_user_id == v.twitch_user_id)
                .map_or(0, |c| c.value);
        }
        Ok(found)
    }

    /// Applies `change` to the marathon clock (loaded first if need be), saves it and announces it. Nothing is kept
    /// in memory when saving fails.
    async fn change_marathon(
        &self,
        change: impl FnOnce(&mut Clock, DateTime<Utc>),
    ) -> Result<MarathonStatus, RepositoryError> {
        let mut guard = self.clock.lock().await;
        let mut clock = match guard.take() {
            Some(clock) => clock,
            None => Clock::new(
                self.repo
                    .load_marathon()
                    .await?
                    .unwrap_or_else(|| Marathon::new(Utc::now())),
            ),
        };
        let before = clock.clone();
        change(&mut clock, Utc::now());
        if let Err(e) = self.repo.save_marathon(&clock.marathon).await {
            *guard = Some(before);
            return Err(e);
        }
        let status = MarathonStatus {
            marathon: clock.marathon.clone(),
            running: clock.running(),
            live: clock.seen == domain::marathon::Seen::Live,
        };
        *guard = Some(clock);
        (self.announce_marathon)(&status);
        Ok(status)
    }

    /// The marathon timer now.
    pub async fn marathon(&self) -> Result<MarathonStatus, RepositoryError> {
        self.change_marathon(|clock, now| clock.settle(now)).await
    }

    pub async fn set_marathon_options(
        &self,
        options: MarathonOptions,
    ) -> Result<MarathonStatus, CounterError> {
        if !options.valid() {
            return Err(CounterError::Invalid);
        }
        Ok(self
            .change_marathon(|clock, now| {
                clock.settle(now);
                clock.marathon.set_options(options);
            })
            .await?)
    }

    /// Pauses or resumes by hand; a stream starting never resumes it.
    pub async fn pause_marathon(&self, paused: bool) -> Result<MarathonStatus, CounterError> {
        Ok(self
            .change_marathon(|clock, now| clock.pause(paused, now))
            .await?)
    }

    /// Adds (or takes) minutes by hand: -1440 to 1440, not 0.
    pub async fn add_to_marathon(&self, minutes: i64) -> Result<MarathonStatus, CounterError> {
        let max = i64::from(domain::marathon::MINUTES_MAX);
        if minutes == 0 || !(-max..=max).contains(&minutes) {
            return Err(CounterError::Invalid);
        }
        Ok(self
            .change_marathon(|clock, now| clock.add(minutes * 60_000, now))
            .await?)
    }

    /// Subs, resubs, gifts and bits add marathon time. Simulated alerts never do, nor a redelivered one; while the
    /// module is disabled they are dropped like other in-memory events.
    pub fn receive_alert(self: &Arc<Self>, alert: &Alert, source: Source) {
        if source == Source::Simulation || !self.modules.is_enabled(modules::MARATHON) {
            return;
        }
        {
            let mut seen = self.seen_alerts.lock().unwrap();
            if seen.contains(&alert.id) {
                return;
            }
            if seen.len() == SEEN_ALERTS {
                seen.pop_front();
            }
            seen.push_back(alert.id.clone());
        }
        let (service, kind) = (self.clone(), alert.kind.clone());
        tokio::spawn(async move {
            let added = service
                .change_marathon(|clock, now| {
                    let ms = clock.marathon.for_alert(&kind);
                    if ms > 0 {
                        clock.add(ms, now);
                    }
                })
                .await;
            if let Err(e) = added {
                tracing::error!(code = "counter.storage_failed", error = %e.0, "marathon time not added");
            }
        });
    }

    /// A stream status: the per-stream reset when live, and the marathon clock either way.
    pub async fn stream_status(&self, state: StreamState) {
        let changed = match state {
            StreamState::Live { started_at } => {
                self.stream_live().await;
                let Some(session) = (self.current_session)().await else {
                    return;
                };
                self.change_marathon(|clock, now| clock.live(&session, started_at, now))
                    .await
            }
            StreamState::Offline => self.change_marathon(|clock, now| clock.offline(now)).await,
        };
        if let Err(e) = changed {
            tracing::error!(code = "counter.storage_failed", error = %e.0, "marathon clock not saved");
        }
    }

    /// The broadcaster is live: per-stream counters not yet reset for this stream's session go back to 0, once
    /// per session, however often this is called (every status update, after a restart, or a late start found by
    /// the minute check).
    pub async fn stream_live(&self) {
        let Some(session) = (self.current_session)().await else {
            return;
        };
        match self.repo.reset_for_session(&session).await {
            Ok(0) => {}
            Ok(_) => self.announce_all().await,
            Err(e) => {
                tracing::error!(code = "counter.storage_failed", error = %e.0, "per-stream reset not stored")
            }
        }
    }
}
