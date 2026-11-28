//! Receives chat messages from Twitch or the simulator and hands them on while the module is enabled; stores the
//! ones from Twitch with the stream session they arrived in, clears what moderators take down, and marks a viewer's
//! first message in the channel.

use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use tokio::{
    sync::{mpsc, oneshot},
    time::{Instant, timeout_at},
};
use toolkit_core::time::{SignedDuration, Timestamp};
use toolkit_core::{
    events::{ChatMessage, ChatRemoval, Source},
    modules::{self, Modules},
};

/// Hands a message to the overlays (and whether it is the sender's first); `infrastructure` publishes it on the
/// bus.
type Publish = Box<dyn Fn(ChatMessage, Source, bool) + Send + Sync>;

/// Tells the overlays and the admin window what a moderator took down.
type PublishRemoval = Box<dyn Fn(&Removal) + Send + Sync>;

/// Tells the admin window that a message already shown is the sender's first.
type PublishFirst = Box<dyn Fn(&str) + Send + Sync>;

/// Asks the stream module for the session a message arriving now belongs to.
pub(crate) type SessionAsk = Arc<dyn Fn() -> SessionAnswer + Send + Sync>;
pub(crate) type SessionAnswer = Pin<Box<dyn Future<Output = Option<String>> + Send>>;

#[derive(Debug, thiserror::Error)]
#[error("chat.storage_failed: {0}")]
pub(crate) struct RepositoryError(pub(crate) String);

/// A message to store: the message, its session, and whether it is the sender's first.
pub(crate) type ToStore = (ChatMessage, Option<String>, bool);

/// Clearing a viewer or the whole chat reaches back this far: Twitch clears what its chat window still shows, not
/// the broadcaster's history. shortcut: a fixed window, not Twitch's own; widen it if older messages show up.
const CLEAR_REACH: SignedDuration = SignedDuration::from_hours(24);

/// What a moderator took down, in which channel (broadcaster user ID), and for a viewer or the whole chat the
/// oldest message it reaches; the admin window and storage use the same boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removal {
    pub channel: String,
    pub removal: ChatRemoval,
    /// `None` for one message, which is taken down whatever its age.
    pub since: Option<Timestamp>,
}

pub(crate) trait ChatRepository: Send + Sync + 'static {
    /// Stores messages in one transaction; one already stored (same Twitch message ID) is left as it is.
    fn store(
        &self,
        messages: &[ToStore],
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
    /// Clears the text of the messages `removal` names, keeping that they were there.
    fn remove(
        &self,
        removal: &Removal,
    ) -> impl Future<Output = Result<u64, RepositoryError>> + Send;
    /// Marks a stored message as its sender's first.
    fn mark_first(&self, id: &str) -> impl Future<Output = Result<(), RepositoryError>> + Send;
}

enum Job {
    Store(Box<ChatMessage>, SessionAnswer, bool),
    Remove(Removal),
    First(String),
    Drained(oneshot::Sender<()>),
}

/// How many messages one write takes at most when chat is busy.
const BATCH: usize = 200;
const BATCH_WINDOW: Duration = Duration::from_millis(75);
// Leave room for moderation even if ordinary chat has filled its share of the queue.
const STORAGE_CAPACITY: usize = 2048;
const MODERATION_RESERVE: usize = 64;
/// How many shown messages a late first-chat mark can still find.
const RECENT_IDS: usize = 500;
/// How many first-chat marks wait for a message that has not arrived yet.
const EARLY_MARKS: usize = 200;
const MAX_RETRY: Duration = Duration::from_secs(30);
const FIRST_RETRY: Duration = Duration::from_millis(500);

/// The IDs of messages shown lately, and first-chat marks that came before their message.
#[derive(Default)]
struct Firsts {
    shown: VecDeque<String>,
    early: VecDeque<String>,
}

pub struct ChatService {
    modules: Arc<Modules>,
    publish: Publish,
    publish_removal: PublishRemoval,
    publish_first: PublishFirst,
    firsts: Mutex<Firsts>,
    write_delayed: Arc<AtomicBool>,
    /// The storage worker and the session question; absent when nothing is stored (tests).
    storage: Option<(mpsc::Sender<Job>, SessionAsk)>,
    missed_writes: AtomicU64,
    stopping: AtomicBool,
    moderation: tokio::sync::Mutex<()>,
}

impl ChatService {
    pub(crate) fn with_publisher(
        modules: Arc<Modules>,
        publish: impl Fn(ChatMessage, Source, bool) + Send + Sync + 'static,
        publish_removal: impl Fn(&Removal) + Send + Sync + 'static,
        publish_first: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        Self {
            modules,
            publish: Box::new(publish),
            publish_removal: Box::new(publish_removal),
            publish_first: Box::new(publish_first),
            firsts: Mutex::default(),
            write_delayed: Arc::default(),
            storage: None,
            missed_writes: AtomicU64::new(0),
            stopping: AtomicBool::new(false),
            moderation: tokio::sync::Mutex::new(()),
        }
    }

    /// Also stores messages from Twitch and applies removals in `repo`, in arrival order, a batch per write; needs a
    /// Tokio runtime.
    pub(crate) fn storing<R: ChatRepository>(mut self, repo: R, ask: SessionAsk) -> Self {
        let (tx, mut rx) = mpsc::channel::<Job>(STORAGE_CAPACITY);
        let delayed = self.write_delayed.clone();
        tokio::spawn(async move {
            while let Some(first) = rx.recv().await {
                let mut jobs = vec![first];
                let deadline = Instant::now() + BATCH_WINDOW;
                while jobs.len() < BATCH && matches!(jobs.last(), Some(Job::Store(..))) {
                    match timeout_at(deadline, rx.recv()).await {
                        Ok(Some(job)) => jobs.push(job),
                        _ => break,
                    }
                }
                let mut batch = Vec::new();
                for job in jobs {
                    match job {
                        Job::Store(message, session, first) => {
                            batch.push((*message, session.await, first));
                        }
                        Job::Remove(removal) => {
                            flush(&repo, &mut batch, &delayed).await;
                            retry(&delayed, "deleted chat not cleared", || {
                                repo.remove(&removal)
                            })
                            .await;
                        }
                        Job::First(id) => {
                            flush(&repo, &mut batch, &delayed).await;
                            retry(&delayed, "first chat not marked", || repo.mark_first(&id)).await;
                        }
                        Job::Drained(done) => {
                            flush(&repo, &mut batch, &delayed).await;
                            let _ = done.send(());
                        }
                    }
                }
                flush(&repo, &mut batch, &delayed).await;
            }
        });
        self.storage = Some((tx, ask));
        self
    }

    /// Dropped while the chat module is disabled. Simulated messages are shown but never stored.
    pub fn receive(&self, message: ChatMessage, source: Source) {
        if !self.modules.is_enabled(modules::CHAT) {
            return;
        }
        // Held until the message is queued and published, so a first-chat mark for it can only follow both.
        let mut firsts = self.firsts.lock().unwrap();
        if self.stopping.load(Ordering::Relaxed) {
            return;
        }
        let early = firsts.early.iter().position(|id| *id == message.id);
        if let Some(i) = early {
            firsts.early.remove(i);
        }
        if firsts.shown.len() == RECENT_IDS {
            firsts.shown.pop_front();
        }
        firsts.shown.push_back(message.id.clone());
        let first = early.is_some();
        if source == Source::Twitch
            && let Some((tx, ask)) = &self.storage
        {
            // Asked now, not when stored, so a stream start that arrived just before counts.
            if tx.capacity() <= MODERATION_RESERVE
                || tx
                    .try_send(Job::Store(Box::new(message.clone()), ask(), first))
                    .is_err()
            {
                self.record_missed_write();
            }
        }
        (self.publish)(message, source, first);
        drop(firsts);
    }

    /// The message with Twitch message ID `id` is its sender's first in the channel. It may come before or after
    /// the message itself; a mark for a message never seen is forgotten after a while.
    pub fn first_chat(&self, id: String) {
        // Held while publishing and queueing, as in `receive`, so the mark never overtakes its message.
        let mut firsts = self.firsts.lock().unwrap();
        if self.stopping.load(Ordering::Relaxed) {
            return;
        }
        if firsts.shown.contains(&id) {
            (self.publish_first)(&id);
            if let Some((tx, _)) = &self.storage
                && (tx.capacity() <= MODERATION_RESERVE || tx.try_send(Job::First(id)).is_err())
            {
                self.record_missed_write();
            }
        } else if !firsts.early.contains(&id) {
            if firsts.early.len() == EARLY_MARKS {
                firsts.early.pop_front();
            }
            firsts.early.push_back(id);
        }
    }

    fn record_missed_write(&self) {
        if self.missed_writes.fetch_add(1, Ordering::Relaxed) == 0 {
            tracing::error!("chat storage queue exhausted; some changes could not be saved");
        }
    }

    pub fn missed_writes(&self) -> u64 {
        self.missed_writes.load(Ordering::Relaxed)
    }

    pub fn write_delayed(&self) -> bool {
        self.write_delayed.load(Ordering::Relaxed)
    }

    /// A moderator took chat down in `channel` (the broadcaster's user ID): overlays and the admin window drop it,
    /// and the stored text is cleared, whether or not the module is enabled.
    pub async fn remove(&self, channel: String, removal: ChatRemoval) {
        let _moderation = self.moderation.lock().await;
        if self.stopping.load(Ordering::Relaxed) {
            return;
        }
        let since = match removal {
            ChatRemoval::Message { .. } => None,
            ChatRemoval::User { .. } | ChatRemoval::All => Some(Timestamp::now() - CLEAR_REACH),
        };
        let removal = Removal {
            channel,
            removal,
            since,
        };
        (self.publish_removal)(&removal);
        if let Some((tx, _)) = &self.storage
            && tx.send(Job::Remove(removal)).await.is_err()
        {
            self.record_missed_write();
        }
    }

    /// Stops taking chat updates and waits up to `within` for all accepted writes to finish; `false` when some are
    /// still pending (a database that keeps failing is retried forever) or the worker is gone.
    pub async fn drain(&self, within: Duration) -> bool {
        tokio::time::timeout(within, async {
            let _moderation = self.moderation.lock().await;
            {
                let _firsts = self.firsts.lock().unwrap();
                self.stopping.store(true, Ordering::Relaxed);
            }
            let Some((tx, _)) = &self.storage else {
                return true;
            };
            let (done, drained) = oneshot::channel();
            tx.send(Job::Drained(done)).await.is_ok() && drained.await.is_ok()
        })
        .await
        .unwrap_or(false)
    }
}

async fn flush<R: ChatRepository>(repo: &R, batch: &mut Vec<ToStore>, delayed: &AtomicBool) {
    if batch.is_empty() {
        return;
    }
    let messages = &*batch;
    retry(delayed, "chat messages not stored", || repo.store(messages)).await;
    batch.clear();
}

/// Runs `write` until it succeeds, with a capped backoff in between, so a database that is
/// briefly locked or failing loses nothing; the jobs behind it wait and keep their order.
async fn retry<T, F: Future<Output = Result<T, RepositoryError>>>(
    delayed: &AtomicBool,
    what: &str,
    mut write: impl FnMut() -> F,
) {
    let mut wait = FIRST_RETRY;
    loop {
        match write().await {
            Ok(_) => {
                delayed.store(false, Ordering::Relaxed);
                return;
            }
            Err(e) => {
                delayed.store(true, Ordering::Relaxed);
                tracing::error!(error = %e, retry_in = ?wait, "{what}; trying again");
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(MAX_RETRY);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        OnceLock,
        atomic::{AtomicU32, Ordering},
    };

    /// Records what reaches storage, in order; the first `fail` writes fail.
    #[derive(Clone, Default)]
    struct FakeRepo {
        fail: Arc<AtomicU32>,
        log: Arc<Mutex<Vec<String>>>,
    }

    impl FakeRepo {
        fn write(&self, what: String) -> Result<(), RepositoryError> {
            let failing = self
                .fail
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok();
            if failing {
                return Err(RepositoryError("locked".into()));
            }
            self.log.lock().unwrap().push(what);
            Ok(())
        }
    }

    impl ChatRepository for FakeRepo {
        async fn store(&self, messages: &[ToStore]) -> Result<(), RepositoryError> {
            let ids: Vec<&str> = messages.iter().map(|(m, ..)| m.id.as_str()).collect();
            self.write(format!("store {}", ids.join(",")))
        }
        async fn remove(&self, removal: &Removal) -> Result<u64, RepositoryError> {
            self.write(format!("remove {:?}", removal.removal))
                .map(|_| 0)
        }
        async fn mark_first(&self, id: &str) -> Result<(), RepositoryError> {
            self.write(format!("first {id}"))
        }
    }

    fn message(id: &str) -> ChatMessage {
        ChatMessage {
            id: id.into(),
            broadcaster_id: "42".into(),
            user_id: "7".into(),
            user_login: "u7".into(),
            user_name: "U7".into(),
            color: None,
            text: "hi".into(),
            fragments: Vec::new(),
            badges: Vec::new(),
            highlighted: false,
            bits: 0,
            reply: None,
            twitch_badges: Vec::new(),
            shared_from: None,
            sent_at: "2026-11-20T10:00:00Z".into(),
        }
    }

    fn modules(name: &str) -> (std::path::PathBuf, Arc<Modules>) {
        let dir = std::env::temp_dir().join(format!("st-chat-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (dir.clone(), Arc::new(Modules::load(dir)))
    }

    async fn wait_for(log: &Mutex<Vec<String>>, n: usize) -> Vec<String> {
        for _ in 0..1000 {
            if log.lock().unwrap().len() >= n {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        log.lock().unwrap().clone()
    }

    #[tokio::test(start_paused = true)]
    async fn a_write_that_fails_for_a_while_is_tried_again_in_order() {
        let (dir, modules) = modules("retry");
        let repo = FakeRepo::default();
        repo.fail.store(2, Ordering::SeqCst);
        let chat = ChatService::with_publisher(modules, |_, _, _| {}, |_| {}, |_| {})
            .storing(repo.clone(), Arc::new(|| Box::pin(async { None })));
        chat.receive(message("m1"), Source::Twitch);
        tokio::time::sleep(Duration::from_millis(1)).await;
        chat.remove("42".into(), ChatRemoval::Message { id: "m1".into() })
            .await;
        assert_eq!(
            wait_for(&repo.log, 2).await,
            ["store m1", "remove Message { id: \"m1\" }"],
            "nothing lost and the removal still comes after the message"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_first_chat_mark_arriving_while_its_message_is_published_never_overtakes_it() {
        let (dir, modules) = modules("order");
        let repo = FakeRepo::default();
        let published = Arc::new(Mutex::new(Vec::new()));
        let (on_message, on_first) = (published.clone(), published.clone());
        let chat: Arc<OnceLock<Arc<ChatService>>> = Arc::default();
        let mark = chat.clone();
        // The session question runs between taking the message in and publishing it: the mark arrives right there,
        // from another thread, and gets a moment to overtake.
        let ask: SessionAsk = Arc::new(move || {
            let chat = mark.get().unwrap().clone();
            let (done_tx, done) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                chat.first_chat("m1".into());
                let _ = done_tx.send(());
            });
            let _ = done.recv_timeout(Duration::from_millis(100));
            Box::pin(async { None })
        });
        let service = ChatService::with_publisher(
            modules,
            move |m, _, _| on_message.lock().unwrap().push(format!("message {}", m.id)),
            |_| {},
            move |id| on_first.lock().unwrap().push(format!("first {id}")),
        )
        .storing(repo.clone(), ask);
        let _ = chat.set(Arc::new(service));
        chat.get().unwrap().receive(message("m1"), Source::Twitch);

        assert_eq!(
            wait_for(&repo.log, 2).await,
            ["store m1", "first m1"],
            "stored before it is marked"
        );
        assert_eq!(*published.lock().unwrap(), ["message m1", "first m1"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(start_paused = true)]
    async fn storage_backlog_is_bounded_and_reserves_space_for_moderation() {
        let (dir, modules) = modules("backlog");
        let repo = FakeRepo::default();
        repo.fail.store(u32::MAX, Ordering::SeqCst);
        let chat = Arc::new(
            ChatService::with_publisher(modules, |_, _, _| {}, |_| {}, |_| {})
                .storing(repo.clone(), Arc::new(|| Box::pin(async { None }))),
        );
        chat.receive(message("blocked"), Source::Twitch);
        tokio::time::sleep(BATCH_WINDOW + Duration::from_millis(1)).await;
        assert!(chat.write_delayed());
        for _ in 0..STORAGE_CAPACITY {
            chat.receive(message("m"), Source::Twitch);
        }
        assert_eq!(chat.missed_writes(), MODERATION_RESERVE as u64);
        for _ in 0..MODERATION_RESERVE {
            chat.remove("42".into(), ChatRemoval::All).await;
        }
        assert_eq!(chat.missed_writes(), MODERATION_RESERVE as u64);
        let removal = tokio::spawn({
            let chat = chat.clone();
            async move { chat.remove("42".into(), ChatRemoval::All).await }
        });
        tokio::task::yield_now().await;
        assert!(!removal.is_finished());
        let shutdown = tokio::spawn({
            let chat = chat.clone();
            async move { chat.drain(Duration::from_secs(3600)).await }
        });
        tokio::task::yield_now().await;
        assert!(!shutdown.is_finished());
        repo.fail.store(0, Ordering::SeqCst);
        removal.await.unwrap();
        assert!(shutdown.await.unwrap());
        assert_eq!(chat.missed_writes(), MODERATION_RESERVE as u64);
        assert_eq!(
            repo.log
                .lock()
                .unwrap()
                .iter()
                .filter(|entry| *entry == "remove All")
                .count(),
            MODERATION_RESERVE + 1
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(start_paused = true)]
    async fn draining_gives_up_when_the_database_keeps_failing() {
        let (dir, modules) = modules("drain-timeout");
        let repo = FakeRepo::default();
        repo.fail.store(u32::MAX, Ordering::SeqCst);
        let chat = ChatService::with_publisher(modules, |_, _, _| {}, |_| {}, |_| {})
            .storing(repo, Arc::new(|| Box::pin(async { None })));
        chat.receive(message("stuck"), Source::Twitch);
        assert!(
            !chat.drain(Duration::from_secs(10)).await,
            "quitting still quits"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_viewer_or_whole_chat_removal_reaches_back_a_day_and_one_message_any_age() {
        let (dir, modules) = modules("since");
        let removals = Arc::new(Mutex::new(Vec::new()));
        let seen = removals.clone();
        let chat = ChatService::with_publisher(
            modules,
            |_, _, _| {},
            move |r: &Removal| seen.lock().unwrap().push(r.clone()),
            |_| {},
        );
        chat.remove("42".into(), ChatRemoval::Message { id: "m1".into() })
            .await;
        chat.remove("42".into(), ChatRemoval::All).await;
        let removals = removals.lock().unwrap();
        assert_eq!(removals[0].since, None);
        assert_eq!(removals[1].channel, "42");
        let reach = Timestamp::now() - removals[1].since.unwrap();
        assert!(
            reach >= CLEAR_REACH && reach < CLEAR_REACH + SignedDuration::from_secs(5),
            "{reach}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
