//! The chat bot (docs/modules/chat-bot.md): answers commands, and posts everything through one rate-limited queue,
//! as the streamer, through a port the Host implements. It never answers its own messages, and simulated events
//! never reach it.

use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use chrono::{DateTime, Utc};
use tokio::{
    sync::mpsc,
    time::{Instant, sleep},
};
use toolkit_core::{
    commands::CommandTable,
    events::{Alert, AlertKind, ChatMessage, Source},
    modules::{self, Modules},
    settings::{self, BotCommand, BotGreetings, BotTimer},
};

use crate::domain::{
    self, Cooldowns, MESSAGE_MAX_CHARS, QUEUE_EXPIRY, QUEUE_MAX, RateWindow, SentMessages, Vars,
    next_due,
};

/// How often due timers are looked at.
const TICK: Duration = Duration::from_secs(1);
/// Chat kept to name the viewer when Twitch's first-chat mark comes after the message: how long, how many.
const RECENT_KEEP: Duration = Duration::from_secs(300);
const RECENT_MAX: usize = 500;
/// `{觀眾}` for an anonymous cheer.
const ANONYMOUS: &str = "匿名";
const DAY: Duration = Duration::from_secs(86_400);

/// The tag on a 回訪 chat line (只提示我): Twitch's message ID and the days since the viewer was last seen.
pub type OnReturning = Arc<dyn Fn(String, u64) + Send + Sync>;

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Posting as the streamer; the Host implements it with Helix Send Chat Message.
pub trait ChatSender: Send + Sync + 'static {
    /// Signed in with `user:write:chat`; without it nothing is queued.
    fn can_send(&self) -> bool;
    /// Posts `text`; Twitch's message ID, or why not (for the log, never with the text).
    fn send(&self, text: String) -> BoxFuture<Result<String, String>>;
}

/// What the bot reads from other modules for its variables; the Host implements it.
pub trait BotLookups: Send + Sync + 'static {
    /// The value of the counter named `name`, if there is one.
    fn counter_value(&self, name: String) -> BoxFuture<Option<i64>>;
}

/// A message waiting to be posted; its variables are filled in when its turn comes.
struct Outgoing {
    template: String,
    viewer: Option<String>,
    amount: Option<u64>,
    queued: Instant,
    /// Another module's text, already filled in: posted as it is, so a viewer's words are never read as variables.
    raw: bool,
}

#[derive(Default)]
struct State {
    rate: RateWindow,
    sent: SentMessages,
    /// The text being posted while Twitch has not answered yet: chat may bring it back before the answer.
    in_flight: Option<String>,
    cooldowns: Cooldowns,
    live_since: Option<DateTime<Utc>>,
    /// The stream's start on the runtime's clock, which timers count from; `None` while not live.
    live_start: Option<Instant>,
    /// Per timer ID: when it is next due, and the chat lines since its last send this stream.
    timers: std::collections::HashMap<String, (Instant, u32)>,
    /// Twitch events already thanked (by event ID), kept like the bot's message IDs.
    thanked: SentMessages,
    /// Viewers greeted as returning this stream (Twitch user IDs).
    returned: std::collections::HashSet<String>,
    /// Recent chat: when, Twitch's message ID, whether the streamer sent it, and the sender's display name.
    recent: std::collections::VecDeque<(Instant, String, bool, String)>,
    /// Messages dropped since launch: the queue was full or they waited too long.
    dropped: u64,
}

/// The key of bot command `id` in the shared command table.
pub(crate) fn command_key(id: &str) -> String {
    format!("bot.{id}")
}

pub struct BotService {
    pub(crate) data_dir: PathBuf,
    pub(crate) commands: Arc<CommandTable>,
    modules: Arc<Modules>,
    sender: Arc<dyn ChatSender>,
    /// What `settings.json` holds, so chat never waits on the file.
    pub(crate) bot_commands: RwLock<Vec<BotCommand>>,
    pub(crate) bot_timers: RwLock<Vec<BotTimer>>,
    pub(crate) greetings: RwLock<BotGreetings>,
    on_returning: OnReturning,
    state: Arc<Mutex<State>>,
    tx: mpsc::Sender<Outgoing>,
}

impl BotService {
    /// Starts the worker that posts the queue in order; needs a Tokio runtime.
    pub fn start(
        data_dir: PathBuf,
        commands: Arc<CommandTable>,
        modules: Arc<Modules>,
        sender: Arc<dyn ChatSender>,
        lookups: Arc<dyn BotLookups>,
        on_returning: OnReturning,
    ) -> Arc<Self> {
        let (tx, rx) = mpsc::channel(QUEUE_MAX);
        let saved = settings::load(&data_dir).bot;
        let service = Arc::new(Self {
            data_dir,
            commands,
            modules: modules.clone(),
            sender: sender.clone(),
            bot_commands: RwLock::new(saved.commands),
            bot_timers: RwLock::new(saved.timers),
            greetings: RwLock::new(saved.greetings),
            on_returning,
            state: Arc::default(),
            tx,
        });
        tokio::spawn(post(rx, service.state.clone(), modules, sender, lookups));
        let ticking = Arc::downgrade(&service);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(TICK);
            loop {
                tick.tick().await;
                let Some(service) = ticking.upgrade() else {
                    return;
                };
                service.post_due_timers();
            }
        });
        service
    }

    fn on(&self) -> bool {
        self.modules.is_enabled(modules::BOT)
    }

    /// Messages dropped since launch because the queue was full or they waited too long.
    pub fn dropped(&self) -> u64 {
        self.state.lock().unwrap().dropped
    }

    /// The streamer posted by hand (the send box): not queued, but it counts toward the same 30 seconds.
    pub fn note_manual_send(&self) {
        self.state
            .lock()
            .unwrap()
            .rate
            .record(Instant::now().into_std());
    }

    /// The stream started (`Some` with its start; repeated by each check) or ended. Timers start counting from the
    /// start of each stream, and stop when it ends.
    pub fn stream_status(&self, live_since: Option<DateTime<Utc>>) {
        let mut state = self.state.lock().unwrap();
        if state.live_since == live_since {
            return;
        }
        state.live_since = live_since;
        state.timers.clear();
        // A new stream greets returning viewers again.
        state.returned.clear();
        state.live_start = live_since.map(|t| {
            let ago = (Utc::now() - t).to_std().unwrap_or_default();
            Instant::now().checked_sub(ago).unwrap_or_else(Instant::now)
        });
        drop(state);
        self.reschedule();
    }

    /// Puts every enabled timer on its next time this stream (keeping the chat lines counted so far); after a
    /// stream starts or the timers change.
    pub(crate) fn reschedule(&self) {
        let mut state = self.state.lock().unwrap();
        let Some(start) = state.live_start else {
            state.timers.clear();
            return;
        };
        let now = Instant::now();
        let timers = self.bot_timers.read().unwrap();
        let mut next = std::collections::HashMap::new();
        for t in timers.iter().filter(|t| t.enabled) {
            let every = Duration::from_secs(u64::from(t.every_minutes) * 60);
            let lines = state.timers.get(&t.id).map_or(0, |(_, l)| *l);
            let due = next_due(start.into_std(), every, now.into_std());
            next.insert(t.id.clone(), (Instant::from_std(due), lines));
        }
        state.timers = next;
    }

    /// When each timer is next due, on the wall clock; empty while not live.
    pub fn next_times(&self) -> std::collections::HashMap<String, DateTime<Utc>> {
        let state = self.state.lock().unwrap();
        let now = Instant::now();
        state
            .timers
            .iter()
            .filter_map(|(id, (due, _))| {
                let left = chrono::Duration::from_std(due.saturating_duration_since(now)).ok()?;
                Some((id.clone(), Utc::now() + left))
            })
            .collect()
    }

    /// Posts each timer that is due, unless chat was too quiet since its last send; either way it waits for its
    /// next interval, with nothing saved up.
    fn post_due_timers(&self) {
        if !self.on() {
            return;
        }
        let now = Instant::now();
        let timers = self.bot_timers.read().unwrap().clone();
        let mut post = Vec::new();
        {
            let mut state = self.state.lock().unwrap();
            let Some(start) = state.live_start else {
                return;
            };
            for t in timers.iter().filter(|t| t.enabled) {
                let Some((due, lines)) = state.timers.get_mut(&t.id) else {
                    continue;
                };
                if now < *due {
                    continue;
                }
                if *lines >= t.min_chat_lines {
                    post.push(t.message.clone());
                    *lines = 0;
                }
                let every = Duration::from_secs(u64::from(t.every_minutes) * 60);
                *due = Instant::from_std(next_due(start.into_std(), every, now.into_std()));
            }
        }
        for message in post {
            self.enqueue(message, None, None);
        }
    }

    /// Queues a message, unless the bot is off or cannot post; a full queue drops it.
    fn enqueue(&self, template: String, viewer: Option<String>, amount: Option<u64>) {
        self.push(Outgoing {
            template,
            viewer,
            amount,
            queued: Instant::now(),
            raw: false,
        });
    }

    /// Posts another module's reply (song requests) as it is, through the same queue and rules: nothing when the bot is off or
    /// cannot post, and its variables are not filled in.
    pub fn say(&self, text: String) {
        self.push(Outgoing {
            template: text,
            viewer: None,
            amount: None,
            queued: Instant::now(),
            raw: true,
        });
    }

    fn push(&self, message: Outgoing) {
        if !self.on() || !self.sender.can_send() {
            return;
        }
        if self.tx.try_send(message).is_err() {
            self.state.lock().unwrap().dropped += 1;
            tracing::warn!(
                code = "bot.dropped",
                reason = "queue_full",
                "chat bot message dropped"
            );
        }
    }

    /// One of the bot's own messages: one it posted (by Twitch's ID), or the one being posted coming back before
    /// Twitch answered (then its ID is kept too).
    fn own(&self, m: &ChatMessage) -> bool {
        let mut state = self.state.lock().unwrap();
        let now = Instant::now().into_std();
        if state.sent.contains(&m.id, now) {
            return true;
        }
        let echo =
            m.user_id == m.broadcaster_id && state.in_flight.as_deref() == Some(m.text.trim());
        if echo {
            state.sent.remember(m.id.clone(), now);
        }
        echo
    }

    /// Whether the bot posted this chat message, so other modules' commands ignore it too.
    pub fn is_own(&self, m: &ChatMessage) -> bool {
        self.own(m)
    }

    /// A chat message from Twitch: counts toward the timers' chat lines and answers the bot command it is, unless
    /// the bot posted it.
    pub fn receive_chat(&self, m: &ChatMessage, source: Source) {
        if source != Source::Twitch || !self.on() || self.own(m) {
            return;
        }
        {
            let mut state = self.state.lock().unwrap();
            for (_, lines) in state.timers.values_mut() {
                *lines = lines.saturating_add(1);
            }
            let now = Instant::now();
            while state
                .recent
                .front()
                .is_some_and(|(t, ..)| now.duration_since(*t) >= RECENT_KEEP)
                || state.recent.len() >= RECENT_MAX
            {
                state.recent.pop_front();
            }
            state.recent.push_back((
                now,
                m.id.clone(),
                m.user_id == m.broadcaster_id,
                m.user_name.clone(),
            ));
        }
        let found = self
            .bot_commands
            .read()
            .unwrap()
            .iter()
            .filter(|c| c.enabled)
            .find(|c| {
                let entry = self.commands.get(&command_key(&c.id));
                entry.matches(&m.text) && entry.allowed.permits(m)
            })
            .cloned();
        let Some(command) = found else {
            return;
        };
        let cooldown = Duration::from_secs(command.cooldown_seconds.into());
        let ready = self.state.lock().unwrap().cooldowns.take(
            &command.id,
            cooldown,
            Instant::now().into_std(),
        );
        if ready {
            self.enqueue(command.reply, Some(m.user_name.clone()), None);
        }
    }
}

impl BotService {
    /// A follow, sub, resub, cheer or raid from Twitch: thanked once (by its event ID) as set. Subs Twitch sends for
    /// each gifted sub are not thanked, nor is the gift itself.
    pub fn receive_alert(&self, a: &Alert, source: Source) {
        if source != Source::Twitch || !self.on() {
            return;
        }
        let g = self.greetings.read().unwrap().clone();
        let (template, amount) = match &a.kind {
            AlertKind::Follow if g.follow.enabled => (g.follow.message, None),
            AlertKind::Subscribe { gifted: false, .. } | AlertKind::Resubscribe { .. }
                if g.sub.enabled =>
            {
                (g.sub.message, None)
            }
            AlertKind::Cheer { bits, .. } if g.bits.enabled && *bits > 0 => {
                let message = match bits {
                    1..=99 => g.bits.small,
                    100..=999 => g.bits.medium,
                    _ => g.bits.large,
                };
                (message, Some(u64::from(*bits)))
            }
            AlertKind::Raid { viewers } if g.raid.enabled => {
                (g.raid.message, Some(u64::from(*viewers)))
            }
            _ => return,
        };
        {
            let mut state = self.state.lock().unwrap();
            let now = Instant::now().into_std();
            if state.thanked.contains(&a.id, now) {
                return;
            }
            state.thanked.remember(a.id.clone(), now);
        }
        let viewer = a.user_name.clone().unwrap_or_else(|| ANONYMOUS.into());
        self.enqueue(template, Some(viewer), amount);
    }

    /// Twitch's first-chat mark (or the toolkit's own first sighting) for message `id`, which may come after the
    /// message: greets its sender, unless it is the streamer.
    pub fn receive_first_chat(&self, id: &str) {
        let g = self.greetings.read().unwrap().first_chat.clone();
        if !self.on() || !g.enabled {
            return;
        }
        let sender = self
            .state
            .lock()
            .unwrap()
            .recent
            .iter()
            .find(|(_, m, ..)| m == id)
            .map(|(_, _, streamer, name)| (*streamer, name.clone()));
        if let Some((false, name)) = sender {
            self.enqueue(g.message, Some(name), None);
        }
    }

    /// A chat message from Twitch with when its sender was last seen before it: a returning viewer's first message
    /// this stream, after the set days, is tagged (只提示我) or greeted, once per stream; never the streamer's.
    pub fn receive_return(&self, m: &ChatMessage, last_seen_before: Option<DateTime<Utc>>) {
        let g = self.greetings.read().unwrap().returning.clone();
        if !self.on() || !g.enabled || m.user_id == m.broadcaster_id {
            return;
        }
        let Some(away) = last_seen_before.and_then(|t| (Utc::now() - t).to_std().ok()) else {
            return;
        };
        let days = away.as_secs() / DAY.as_secs();
        if away < DAY * g.days {
            return;
        }
        {
            let mut state = self.state.lock().unwrap();
            if state.live_start.is_none() || !state.returned.insert(m.user_id.clone()) {
                return;
            }
        }
        if g.post {
            self.enqueue(g.message, Some(m.user_name.clone()), None);
        } else {
            (self.on_returning)(m.id.clone(), days);
        }
    }
}

/// Posts the queue in order: drops what waited too long or arrives with the bot off, keeps to the rate, fills in
/// the variables, and remembers each posted message's ID.
async fn post(
    mut rx: mpsc::Receiver<Outgoing>,
    state: Arc<Mutex<State>>,
    modules: Arc<Modules>,
    sender: Arc<dyn ChatSender>,
    lookups: Arc<dyn BotLookups>,
) {
    let expired = |m: &Outgoing| m.queued.elapsed() > QUEUE_EXPIRY;
    while let Some(message) = rx.recv().await {
        loop {
            let wait = state.lock().unwrap().rate.wait(Instant::now().into_std());
            if wait.is_zero() || expired(&message) {
                break;
            }
            sleep(wait).await;
        }
        if !modules.is_enabled(modules::BOT) || !sender.can_send() {
            continue;
        }
        if expired(&message) {
            state.lock().unwrap().dropped += 1;
            tracing::warn!(
                code = "bot.dropped",
                reason = "expired",
                "chat bot message dropped"
            );
            continue;
        }
        let text = render(&message, &state, lookups.as_ref()).await;
        let Some(text) = text else {
            continue;
        };
        state.lock().unwrap().in_flight = Some(text.clone());
        let result = sender.send(text).await;
        let mut s = state.lock().unwrap();
        let now = Instant::now().into_std();
        // Kept before the text is let go, so its echo is always known one way or the other.
        if let Ok(id) = &result {
            s.sent.remember(id.clone(), now);
        }
        s.in_flight = None;
        s.rate.record(now);
        drop(s);
        if let Err(e) = result {
            tracing::warn!(code = "bot.send_failed", error = %e, "chat bot message not posted");
        }
    }
}

/// The message's text with its variables filled in now.
async fn render(m: &Outgoing, state: &Mutex<State>, lookups: &dyn BotLookups) -> Option<String> {
    if m.raw {
        let text = m.template.trim();
        return (!text.is_empty()).then(|| text.chars().take(MESSAGE_MAX_CHARS).collect());
    }
    // Counter values are read first: rendering itself is not async.
    let mut values = Vec::new();
    for name in counter_names(&m.template) {
        if let Some(v) = lookups.counter_value(name.clone()).await {
            values.push((name, v));
        }
    }
    let live_for = state
        .lock()
        .unwrap()
        .live_since
        .and_then(|t| (Utc::now() - t).to_std().ok());
    let counters = |name: &str| values.iter().find(|(n, _)| n == name).map(|(_, v)| *v);
    domain::render(
        &m.template,
        &Vars {
            viewer: m.viewer.as_deref(),
            amount: m.amount,
            counters: &counters,
            live_for,
        },
    )
}

/// The names in a message's `{計數:名稱}` variables.
fn counter_names(template: &str) -> Vec<String> {
    template
        .split("{計數:")
        .skip(1)
        .filter_map(|rest| rest.split_once('}').map(|(name, _)| name.to_owned()))
        .collect()
}

#[cfg(test)]
mod tests;
