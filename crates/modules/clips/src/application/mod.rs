//! The clip queue: clips are looked up when added and wait in memory (a restart empties the queue).

use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

use crate::domain::{Clip, ClipId, QUEUE_MAX, parse_clip_id};

/// Why a lookup gave no clip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupError {
    NotFound,
    /// A Twitch or sign-in error, with the HTTP status and `twitch.*` code the host maps it to.
    Twitch {
        status: u16,
        code: String,
    },
}

/// Looks a clip up by ID; the host implements it with Helix Get Clips.
pub trait ClipLookup: Send + Sync + 'static {
    fn clip<'a>(
        &'a self,
        id: &'a ClipId,
    ) -> Pin<Box<dyn Future<Output = Result<Clip, LookupError>> + Send + 'a>>;
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ClipError {
    #[error("clip.invalid_input")]
    InvalidInput,
    #[error("clip.not_found")]
    NotFound,
    #[error("clip.queue_full")]
    QueueFull,
    #[error("{code}")]
    Twitch { status: u16, code: String },
}

impl From<LookupError> for ClipError {
    fn from(e: LookupError) -> Self {
        match e {
            LookupError::NotFound => Self::NotFound,
            LookupError::Twitch { status, code } => Self::Twitch { status, code },
        }
    }
}

/// A queued clip. `entry` tells apart the same clip queued twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub entry: String,
    pub clip: Clip,
}

pub struct ClipsService {
    lookup: Arc<dyn ClipLookup>,
    queue: Mutex<VecDeque<Entry>>,
}

impl ClipsService {
    pub fn new(lookup: Arc<dyn ClipLookup>) -> Self {
        Self {
            lookup,
            queue: Mutex::new(VecDeque::new()),
        }
    }

    fn queue(&self) -> std::sync::MutexGuard<'_, VecDeque<Entry>> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn list(&self) -> Vec<Entry> {
        self.queue().iter().cloned().collect()
    }

    /// Looks the clip up and queues it at the end.
    pub async fn add(&self, input: &str) -> Result<Entry, ClipError> {
        let id = parse_clip_id(input).ok_or(ClipError::InvalidInput)?;
        // Checked before the lookup to spare a Helix call, and again below because adds run concurrently.
        if self.queue().len() >= QUEUE_MAX {
            return Err(ClipError::QueueFull);
        }
        let clip = self.lookup.clip(&id).await?;
        let entry = Entry {
            entry: uuid::Uuid::now_v7().to_string(),
            clip,
        };
        let mut queue = self.queue();
        if queue.len() >= QUEUE_MAX {
            return Err(ClipError::QueueFull);
        }
        queue.push_back(entry.clone());
        Ok(entry)
    }

    /// Removing an entry that is not queued does nothing.
    pub fn remove(&self, entry: &str) {
        self.queue().retain(|e| e.entry != entry);
    }

    pub fn clear(&self) {
        self.queue().clear();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Knows `known` (30 s long) only; `failure` overrides every answer.
    pub(crate) struct FakeLookup {
        pub(crate) failure: Option<LookupError>,
        pub(crate) calls: AtomicUsize,
    }

    pub(crate) const KNOWN: &str = "CrispyJollyGullHassaanChop-nPlLKGxGRcBj37e4";

    impl FakeLookup {
        pub(crate) fn new(failure: Option<LookupError>) -> Arc<Self> {
            Arc::new(Self {
                failure,
                calls: AtomicUsize::new(0),
            })
        }
    }

    impl ClipLookup for FakeLookup {
        fn clip<'a>(
            &'a self,
            id: &'a ClipId,
        ) -> Pin<Box<dyn Future<Output = Result<Clip, LookupError>> + Send + 'a>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                // Gives way once, like a real request, so concurrent adds interleave in tests.
                tokio::task::yield_now().await;
                if let Some(e) = &self.failure {
                    return Err(e.clone());
                }
                if id.as_str() != KNOWN {
                    return Err(LookupError::NotFound);
                }
                Ok(Clip {
                    id: id.as_str().into(),
                    title: "F1 2017 E3 Gameplay!".into(),
                    broadcaster_name: "Twitch".into(),
                    duration_ms: 30_000,
                })
            })
        }
    }

    #[tokio::test]
    async fn adds_look_the_clip_up_and_queue_it_in_order() {
        let service = ClipsService::new(FakeLookup::new(None));
        let first = service
            .add(&format!("https://clips.twitch.tv/{KNOWN}"))
            .await
            .unwrap();
        assert_eq!(first.clip.duration_ms, 30_000);
        let second = service.add(KNOWN).await.unwrap();
        assert_ne!(
            first.entry, second.entry,
            "the same clip twice is two entries"
        );
        assert_eq!(service.list(), vec![first, second]);
    }

    #[tokio::test]
    async fn refused_adds_leave_the_queue_alone() {
        let lookup = FakeLookup::new(None);
        let service = ClipsService::new(lookup.clone());
        assert_eq!(
            service.add("https://example.com/x").await,
            Err(ClipError::InvalidInput)
        );
        assert_eq!(
            lookup.calls.load(Ordering::SeqCst),
            0,
            "invalid input is not looked up"
        );
        assert_eq!(service.add("GoneClip").await, Err(ClipError::NotFound));

        let twitch = LookupError::Twitch {
            status: 409,
            code: "twitch.sign_in_required".into(),
        };
        let signed_out = ClipsService::new(FakeLookup::new(Some(twitch)));
        let e = signed_out.add(KNOWN).await.unwrap_err();
        assert_eq!(e.to_string(), "twitch.sign_in_required");
        assert!(service.list().is_empty() && signed_out.list().is_empty());
    }

    #[tokio::test]
    async fn the_queue_holds_at_most_50() {
        let lookup = FakeLookup::new(None);
        let service = ClipsService::new(lookup.clone());
        for _ in 0..QUEUE_MAX {
            service.add(KNOWN).await.unwrap();
        }
        assert_eq!(service.add(KNOWN).await, Err(ClipError::QueueFull));
        assert_eq!(
            lookup.calls.load(Ordering::SeqCst),
            QUEUE_MAX,
            "a full queue skips the lookup"
        );
        assert_eq!(service.list().len(), QUEUE_MAX);
    }

    #[tokio::test]
    async fn concurrent_adds_never_pass_the_cap() {
        let service = ClipsService::new(FakeLookup::new(None));
        for _ in 0..QUEUE_MAX - 1 {
            service.add(KNOWN).await.unwrap();
        }
        // Both pass the check before the lookup; only the check at insert stops the second.
        let (a, b) = tokio::join!(service.add(KNOWN), service.add(KNOWN));
        assert_eq!(
            [a.is_ok(), b.is_ok()].iter().filter(|ok| **ok).count(),
            1,
            "{a:?} {b:?}"
        );
        assert!([a, b].contains(&Err(ClipError::QueueFull)));
        assert_eq!(service.list().len(), QUEUE_MAX);
    }

    #[tokio::test]
    async fn remove_and_clear() {
        let service = ClipsService::new(FakeLookup::new(None));
        let a = service.add(KNOWN).await.unwrap();
        let b = service.add(KNOWN).await.unwrap();
        service.remove("not-queued");
        service.remove(&a.entry);
        assert_eq!(service.list(), vec![b]);
        service.clear();
        assert!(service.list().is_empty());
    }
}
