//! Receives chat messages from Twitch or the simulator and hands them on while the module is enabled; stores the
//! ones from Twitch with the stream session they arrived in, and clears what moderators take down.

use std::{future::Future, pin::Pin, sync::Arc};

use tokio::sync::mpsc;
use toolkit_core::{
    events::{ChatMessage, ChatRemoval, Source},
    modules::{self, Modules},
};

/// Hands a message to the overlays; `infrastructure` publishes it on the bus.
type Publish = Box<dyn Fn(ChatMessage, Source) + Send + Sync>;

/// Tells the overlays and the admin window what a moderator took down.
type PublishRemoval = Box<dyn Fn(&ChatRemoval) + Send + Sync>;

/// Asks the stream module for the session a message arriving now belongs to.
pub(crate) type SessionAsk = Arc<dyn Fn() -> SessionAnswer + Send + Sync>;
pub(crate) type SessionAnswer = Pin<Box<dyn Future<Output = Option<String>> + Send>>;

#[derive(Debug, thiserror::Error)]
#[error("chat.storage_failed: {0}")]
pub(crate) struct RepositoryError(pub(crate) String);

pub(crate) trait ChatRepository: Send + Sync + 'static {
    /// Stores messages in one transaction; one already stored (same Twitch message ID) is left as it is.
    fn store(
        &self,
        messages: &[(ChatMessage, Option<String>)],
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
    /// Clears the text of the messages `removal` names, keeping that they were there.
    fn remove(
        &self,
        removal: &ChatRemoval,
    ) -> impl Future<Output = Result<u64, RepositoryError>> + Send;
}

enum Job {
    Store(Box<ChatMessage>, SessionAnswer),
    Remove(ChatRemoval),
}

/// How many messages one write takes at most when chat is busy.
const BATCH: usize = 200;

pub struct ChatService {
    modules: Arc<Modules>,
    publish: Publish,
    publish_removal: PublishRemoval,
    /// The storage worker and the session question; absent when nothing is stored (tests).
    storage: Option<(mpsc::UnboundedSender<Job>, SessionAsk)>,
}

impl ChatService {
    pub(crate) fn with_publisher(
        modules: Arc<Modules>,
        publish: impl Fn(ChatMessage, Source) + Send + Sync + 'static,
        publish_removal: impl Fn(&ChatRemoval) + Send + Sync + 'static,
    ) -> Self {
        Self {
            modules,
            publish: Box::new(publish),
            publish_removal: Box::new(publish_removal),
            storage: None,
        }
    }

    /// Also stores messages from Twitch and applies removals in `repo`, in arrival order, a batch per write; needs a
    /// Tokio runtime.
    pub(crate) fn storing<R: ChatRepository>(mut self, repo: R, ask: SessionAsk) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<Job>();
        tokio::spawn(async move {
            while let Some(first) = rx.recv().await {
                let mut jobs = vec![first];
                while jobs.len() < BATCH {
                    match rx.try_recv() {
                        Ok(job) => jobs.push(job),
                        Err(_) => break,
                    }
                }
                let mut batch = Vec::new();
                for job in jobs {
                    match job {
                        Job::Store(message, session) => batch.push((*message, session.await)),
                        Job::Remove(removal) => {
                            flush(&repo, &mut batch).await;
                            if let Err(e) = repo.remove(&removal).await {
                                tracing::error!(error = %e, "deleted chat not cleared");
                            }
                        }
                    }
                }
                flush(&repo, &mut batch).await;
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
        if source == Source::Twitch
            && let Some((tx, ask)) = &self.storage
        {
            // Asked now, not when stored, so a stream start that arrived just before counts.
            let _ = tx.send(Job::Store(Box::new(message.clone()), ask()));
        }
        (self.publish)(message, source);
    }

    /// A moderator took chat down: overlays and the admin window drop it, and the stored text is cleared, whether or
    /// not the module is enabled.
    pub fn remove(&self, removal: ChatRemoval) {
        (self.publish_removal)(&removal);
        if let Some((tx, _)) = &self.storage {
            let _ = tx.send(Job::Remove(removal));
        }
    }
}

async fn flush<R: ChatRepository>(repo: &R, batch: &mut Vec<(ChatMessage, Option<String>)>) {
    if batch.is_empty() {
        return;
    }
    if let Err(e) = repo.store(batch).await {
        tracing::error!(error = %e, count = batch.len(), "chat messages not stored");
    }
    batch.clear();
}
