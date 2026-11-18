//! The clip queue and its player. Clips are looked up when added and wait in memory (a restart empties the queue).
//! One player task owns the timing: it estimates each clip's end from its Helix duration, because the official
//! clip embed reports nothing back (no start, end or error events).

use std::{
    collections::VecDeque,
    future::Future,
    io,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use tokio::{
    sync::mpsc,
    time::{Instant, sleep_until},
};
use toolkit_core::{
    modules::{self, Modules},
    settings,
};

use crate::domain::{Clip, ClipId, QUEUE_MAX, parse_clip_id};

/// Time the overlay gets to load the embed before the clip's duration starts counting.
const LOAD_ALLOWANCE: Duration = Duration::from_secs(3);
/// Pause on a clip that no longer exists before the next one.
const ERROR_PAUSE: Duration = Duration::from_secs(2);
/// Largest switch buffer the broadcaster can set, in seconds.
const BUFFER_MAX: u32 = 30;

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

#[derive(Debug, thiserror::Error)]
pub enum ClipError {
    #[error("clip.invalid_input")]
    InvalidInput,
    #[error("clip.not_found")]
    NotFound,
    #[error("clip.queue_full")]
    QueueFull,
    #[error("clip.invalid_buffer")]
    InvalidBuffer,
    #[error("settings.save_failed")]
    SaveFailed(#[from] io::Error),
    #[error("{code}")]
    Twitch { status: u16, code: String },
}

/// Errors are equal when their codes are (`io::Error` has no equality of its own).
impl PartialEq for ClipError {
    fn eq(&self, other: &Self) -> bool {
        self.to_string() == other.to_string()
    }
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

/// What the player is doing. Only `Error` comes from something the Host saw; the playing states are estimates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Loading,
    EstimatedPlaying,
    Transitioning,
    Stopped,
    Error,
}

impl State {
    fn active(self) -> bool {
        matches!(
            self,
            Self::Loading | Self::EstimatedPlaying | Self::Transitioning | Self::Error
        )
    }
}

/// The player's state for the admin UI and the overlays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playback {
    pub state: State,
    /// New for every play; kept in `Idle` and `Stopped`; `None` only before the first clip.
    pub session_id: Option<String>,
    /// The current clip, or the last one in `Stopped`; `None` in `Idle`.
    pub clip: Option<Clip>,
    /// Estimated, from the start of `EstimatedPlaying`; the clip's duration in `Transitioning`; else `None`.
    pub elapsed_ms: Option<u64>,
}

/// Hands the player's state to the overlays; `infrastructure` publishes it on the bus.
type Publish = Box<dyn Fn(&Playback) + Send + Sync>;

enum Command {
    /// A clip was queued: start it if nothing is playing.
    Added,
    Skip(String),
    Stop,
    Play,
    Replay(String),
    /// The lookup made when a clip starts; dropped when its session is over.
    Looked {
        session: String,
        result: Result<Clip, LookupError>,
    },
}

struct Shared {
    queue: VecDeque<Entry>,
    state: State,
    session_id: Option<String>,
    /// The current clip, or the last one played; replay plays it again.
    clip: Option<Clip>,
    phase_started: Instant,
}

struct Inner {
    lookup: Arc<dyn ClipLookup>,
    modules: Arc<Modules>,
    data_dir: PathBuf,
    publish: Publish,
    shared: Mutex<Shared>,
    commands: mpsc::UnboundedSender<Command>,
}

impl Inner {
    fn shared(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn playback(&self) -> Playback {
        let s = self.shared();
        let elapsed_ms = match (s.state, &s.clip) {
            (State::EstimatedPlaying, _) => Some(s.phase_started.elapsed().as_millis() as u64),
            (State::Transitioning, Some(clip)) => Some(clip.duration_ms),
            _ => None,
        };
        Playback {
            state: s.state,
            session_id: s.session_id.clone(),
            clip: (s.state != State::Idle).then(|| s.clip.clone()).flatten(),
            elapsed_ms,
        }
    }

    fn buffer(&self) -> u32 {
        settings::load(&self.data_dir)
            .clips
            .buffer_secs
            .min(BUFFER_MAX)
    }

    fn send(&self, command: Command) {
        // The player task holds the receiver for the life of the service, so sending cannot fail.
        let _ = self.commands.send(command);
    }
}

pub struct ClipsService(Arc<Inner>);

impl ClipsService {
    /// Starts the player task; needs a Tokio runtime. Publishes `Idle` at once so overlays always get a state.
    pub(crate) fn with_publisher(
        lookup: Arc<dyn ClipLookup>,
        modules: Arc<Modules>,
        data_dir: PathBuf,
        publish: impl Fn(&Playback) + Send + Sync + 'static,
    ) -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        let inner = Arc::new(Inner {
            lookup,
            modules,
            data_dir,
            publish: Box::new(publish),
            shared: Mutex::new(Shared {
                queue: VecDeque::new(),
                state: State::Idle,
                session_id: None,
                clip: None,
                phase_started: Instant::now(),
            }),
            commands,
        });
        (inner.publish)(&inner.playback());
        tokio::spawn(
            Player {
                inner: inner.clone(),
                deadline: None,
                loaded: false,
                looked: false,
            }
            .run(receiver),
        );
        Self(inner)
    }

    pub fn list(&self) -> Vec<Entry> {
        self.0.shared().queue.iter().cloned().collect()
    }

    pub fn playback(&self) -> Playback {
        self.0.playback()
    }

    /// Looks the clip up and queues it at the end; an idle player starts it.
    pub async fn add(&self, input: &str) -> Result<Entry, ClipError> {
        let id = parse_clip_id(input).ok_or(ClipError::InvalidInput)?;
        // Checked before the lookup to spare a Helix call, and again below because adds run concurrently.
        if self.0.shared().queue.len() >= QUEUE_MAX {
            return Err(ClipError::QueueFull);
        }
        let clip = self.0.lookup.clip(&id).await?;
        let entry = Entry {
            entry: uuid::Uuid::now_v7().to_string(),
            clip,
        };
        let mut shared = self.0.shared();
        if shared.queue.len() >= QUEUE_MAX {
            return Err(ClipError::QueueFull);
        }
        shared.queue.push_back(entry.clone());
        drop(shared);
        self.0.send(Command::Added);
        Ok(entry)
    }

    /// Removing an entry that is not queued does nothing. The playing clip has already left the queue.
    pub fn remove(&self, entry: &str) {
        self.0.shared().queue.retain(|e| e.entry != entry);
    }

    /// Empties the queue; the playing clip plays on.
    pub fn clear(&self) {
        self.0.shared().queue.clear();
    }

    /// Moves on to the next clip if `session_id` is still the current one, so a repeated skip moves on once.
    pub fn skip(&self, session_id: &str) {
        self.0.send(Command::Skip(session_id.into()));
    }

    pub fn stop(&self) {
        self.0.send(Command::Stop);
    }

    /// Plays the next queued clip after a stop, or when idle.
    pub fn play(&self) {
        self.0.send(Command::Play);
    }

    /// Plays the current or last clip again if `session_id` is still the current one.
    pub fn replay(&self, session_id: &str) {
        self.0.send(Command::Replay(session_id.into()));
    }

    pub fn buffer_secs(&self) -> u32 {
        self.0.buffer()
    }

    pub fn set_buffer_secs(&self, secs: u32) -> Result<(), ClipError> {
        if secs > BUFFER_MAX {
            return Err(ClipError::InvalidBuffer);
        }
        settings::update(&self.0.data_dir, |s| s.clips.buffer_secs = secs)?;
        Ok(())
    }
}

/// The player task: the only place that changes the state, so no two timers ever run.
struct Player {
    inner: Arc<Inner>,
    deadline: Option<Instant>,
    /// The load allowance of the current clip has passed.
    loaded: bool,
    /// The lookup of the current clip has answered.
    looked: bool,
}

impl Player {
    async fn run(mut self, mut commands: mpsc::UnboundedReceiver<Command>) {
        loop {
            let deadline = self.deadline;
            tokio::select! {
                command = commands.recv() => match command {
                    Some(command) => self.command(command),
                    None => return,
                },
                () = sleep_until(deadline.unwrap_or_else(Instant::now)), if deadline.is_some() => {
                    self.deadline = None;
                    self.timeout();
                }
            }
        }
    }

    fn set(&mut self, state: State, clip: Option<Clip>, new_session: bool) {
        let mut s = self.inner.shared();
        s.state = state;
        if clip.is_some() {
            s.clip = clip;
        }
        if new_session {
            s.session_id = Some(uuid::Uuid::now_v7().to_string());
        }
        s.phase_started = Instant::now();
        drop(s);
        (self.inner.publish)(&self.inner.playback());
    }

    fn state(&self) -> State {
        self.inner.shared().state
    }

    fn current_session(&self, session_id: &str) -> bool {
        self.inner.shared().session_id.as_deref() == Some(session_id)
    }

    fn command(&mut self, command: Command) {
        let state = self.state();
        match command {
            Command::Added if state == State::Idle => self.next(),
            Command::Skip(id) if state.active() && self.current_session(&id) => self.next(),
            Command::Stop => self.halt(),
            Command::Play
                if matches!(state, State::Idle | State::Stopped)
                    && !self.inner.shared().queue.is_empty() =>
            {
                self.next()
            }
            Command::Replay(id) if self.current_session(&id) => {
                let last = self.inner.shared().clip.clone();
                if let Some(clip) = last {
                    self.start(clip);
                }
            }
            Command::Looked { session, result }
                if state == State::Loading && self.current_session(&session) =>
            {
                self.looked = true;
                match result {
                    Err(LookupError::NotFound) => {
                        self.set(State::Error, None, false);
                        self.deadline = Some(Instant::now() + ERROR_PAUSE);
                        return;
                    }
                    Err(LookupError::Twitch { code, .. }) => {
                        // Signed out or Twitch unreachable: play with the data saved when it was queued.
                        tracing::warn!(code = %code, "clip lookup failed; playing the queued data");
                    }
                    // The saved duration stays; only a deleted clip changes anything.
                    Ok(_) => {}
                }
                if self.loaded {
                    self.playing();
                }
            }
            _ => {}
        }
    }

    fn timeout(&mut self) {
        match self.state() {
            State::Loading => {
                self.loaded = true;
                if self.looked {
                    self.playing();
                }
            }
            State::EstimatedPlaying => {
                self.set(State::Transitioning, None, false);
                let buffer = Duration::from_secs(self.inner.buffer().into());
                self.deadline = Some(Instant::now() + buffer);
            }
            State::Transitioning | State::Error => self.next(),
            State::Idle | State::Stopped => {}
        }
    }

    /// Stops a playing clip; it stays as the last clip for replay. Does nothing unless a clip is playing.
    fn halt(&mut self) {
        if self.state().active() {
            self.deadline = None;
            self.set(State::Stopped, None, false);
        }
    }

    fn enabled(&self) -> bool {
        self.inner.modules.is_enabled(modules::CLIPS)
    }

    /// Starts the next queued clip, or goes idle when there is none. A disabled module starts nothing.
    fn next(&mut self) {
        if !self.enabled() {
            return self.halt();
        }
        let next = self.inner.shared().queue.pop_front();
        match next {
            Some(entry) => self.start(entry.clip),
            None => {
                self.deadline = None;
                self.inner.shared().clip = None;
                self.set(State::Idle, None, false);
            }
        }
    }

    fn start(&mut self, clip: Clip) {
        if !self.enabled() {
            return self.halt();
        }
        self.loaded = false;
        self.deadline = Some(Instant::now() + LOAD_ALLOWANCE);
        let id = parse_clip_id(&clip.id);
        self.looked = id.is_none();
        self.set(State::Loading, Some(clip), true);
        let Some(id) = id else { return };
        let session = self.inner.shared().session_id.clone().unwrap_or_default();
        // The lookup runs beside the loop, so skip and stop never wait for Twitch.
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let result = inner.lookup.clip(&id).await;
            inner.send(Command::Looked { session, result });
        });
    }

    fn playing(&mut self) {
        let duration = self
            .inner
            .shared()
            .clip
            .as_ref()
            .map_or(0, |c| c.duration_ms);
        self.set(State::EstimatedPlaying, None, false);
        self.deadline = Some(Instant::now() + Duration::from_millis(duration));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Knows `KNOWN` (30 s long) only; `failure` overrides every answer from then on.
    pub(crate) struct FakeLookup {
        failure: Mutex<Option<LookupError>>,
        pub(crate) calls: AtomicUsize,
    }

    pub(crate) const KNOWN: &str = "CrispyJollyGullHassaanChop-nPlLKGxGRcBj37e4";

    impl FakeLookup {
        pub(crate) fn new(failure: Option<LookupError>) -> Arc<Self> {
            Arc::new(Self {
                failure: Mutex::new(failure),
                calls: AtomicUsize::new(0),
            })
        }

        fn fail(&self, e: LookupError) {
            *self.failure.lock().unwrap() = Some(e);
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
                if let Some(e) = self.failure.lock().unwrap().clone() {
                    return Err(e);
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

    pub(crate) fn data_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-clips-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A service whose published states are recorded.
    fn service(name: &str, lookup: Arc<FakeLookup>) -> (ClipsService, Arc<Mutex<Vec<Playback>>>) {
        let dir = data_dir(name);
        let published = Arc::new(Mutex::new(Vec::new()));
        let record = published.clone();
        let service = ClipsService::with_publisher(
            lookup,
            Arc::new(Modules::load(dir.clone())),
            dir,
            move |p| record.lock().unwrap().push(p.clone()),
        );
        (service, published)
    }

    /// Lets the player task catch up, `ms` of paused time later.
    async fn after(ms: u64) {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }

    fn session(service: &ClipsService) -> String {
        service.playback().session_id.unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn adds_look_the_clip_up_and_queue_it_in_order() {
        let (service, _) = service("order", FakeLookup::new(None));
        service.stop(); // a stop while idle changes nothing
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
        after(1).await;
        assert_eq!(
            service.list(),
            vec![second],
            "the first one started playing"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn refused_adds_leave_the_queue_alone() {
        let lookup = FakeLookup::new(None);
        let (service, _) = service("refused", lookup.clone());
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
        let (signed_out, _) = self::service("refused-out", FakeLookup::new(Some(twitch)));
        let e = signed_out.add(KNOWN).await.unwrap_err();
        assert_eq!(e.to_string(), "twitch.sign_in_required");
        after(1).await;
        assert!(service.list().is_empty() && signed_out.list().is_empty());
        assert_eq!(service.playback().state, State::Idle);
    }

    #[tokio::test(start_paused = true)]
    async fn the_queue_holds_at_most_50() {
        let lookup = FakeLookup::new(None);
        let (service, _) = service("cap", lookup.clone());
        service.add(KNOWN).await.unwrap(); // starts playing and leaves the queue
        after(1).await;
        for _ in 0..QUEUE_MAX {
            service.add(KNOWN).await.unwrap();
        }
        let calls = lookup.calls.load(Ordering::SeqCst);
        assert_eq!(service.add(KNOWN).await, Err(ClipError::QueueFull));
        assert_eq!(
            lookup.calls.load(Ordering::SeqCst),
            calls,
            "a full queue skips the lookup"
        );
        assert_eq!(service.list().len(), QUEUE_MAX);
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_adds_never_pass_the_cap() {
        let (service, _) = service("concurrent", FakeLookup::new(None));
        service.add(KNOWN).await.unwrap(); // starts playing and leaves the queue
        after(1).await;
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
        assert!(
            [a, b]
                .iter()
                .any(|r| r.as_ref().err() == Some(&ClipError::QueueFull))
        );
        assert_eq!(service.list().len(), QUEUE_MAX);
    }

    #[tokio::test(start_paused = true)]
    async fn remove_and_clear_leave_the_playing_clip_alone() {
        let (service, _) = service("remove", FakeLookup::new(None));
        service.add(KNOWN).await.unwrap();
        after(1).await;
        let a = service.add(KNOWN).await.unwrap();
        let b = service.add(KNOWN).await.unwrap();
        service.remove("not-queued");
        service.remove(&a.entry);
        assert_eq!(service.list(), vec![b]);
        service.clear();
        after(1).await;
        assert!(service.list().is_empty());
        assert_eq!(service.playback().state, State::Loading);
    }

    #[tokio::test(start_paused = true)]
    async fn clips_switch_after_load_duration_and_buffer() {
        let (service, published) = service("timeline", FakeLookup::new(None));
        assert_eq!(
            published.lock().unwrap()[0].state,
            State::Idle,
            "published at start"
        );
        service.add(KNOWN).await.unwrap();
        service.add(KNOWN).await.unwrap();
        after(1).await;
        let first = service.playback();
        assert_eq!(first.state, State::Loading);
        assert_eq!(first.clip.as_ref().unwrap().id, KNOWN);
        assert_eq!(first.elapsed_ms, None);

        after(3_000).await; // load allowance
        assert_eq!(service.playback().state, State::EstimatedPlaying);
        after(10_000).await;
        let elapsed = service.playback().elapsed_ms.unwrap();
        assert!((9_990..=10_010).contains(&elapsed), "{elapsed}");

        after(20_000).await; // the 30 s duration is over
        let transitioning = service.playback();
        assert_eq!(transitioning.state, State::Transitioning);
        assert_eq!(transitioning.elapsed_ms, Some(30_000));
        after(2_000).await; // default buffer
        let second = service.playback();
        assert_eq!(second.state, State::Loading);
        assert_ne!(
            second.session_id, first.session_id,
            "every play has a new session"
        );

        after(35_000).await;
        let idle = service.playback();
        assert_eq!((idle.state, idle.clip), (State::Idle, None));
        assert_eq!(
            idle.session_id, second.session_id,
            "idle keeps the last session"
        );
        let states: Vec<State> = published.lock().unwrap().iter().map(|p| p.state).collect();
        use State::*;
        assert_eq!(
            states,
            [
                Idle,
                Loading,
                EstimatedPlaying,
                Transitioning,
                Loading,
                EstimatedPlaying,
                Transitioning,
                Idle
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_buffer_setting_is_checked_and_used() {
        let (service, _) = service("buffer", FakeLookup::new(None));
        assert_eq!(service.buffer_secs(), 2);
        assert_eq!(service.set_buffer_secs(31), Err(ClipError::InvalidBuffer));
        service.set_buffer_secs(0).unwrap();
        assert_eq!(service.buffer_secs(), 0);
        service.add(KNOWN).await.unwrap();
        after(33_001).await;
        assert_eq!(
            service.playback().state,
            State::Idle,
            "no buffer after the clip"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_stale_skip_does_nothing_so_a_double_click_skips_once() {
        let (service, _) = service("skip", FakeLookup::new(None));
        for _ in 0..3 {
            service.add(KNOWN).await.unwrap();
        }
        after(1).await;
        let first = session(&service);
        service.skip(&first);
        service.skip(&first);
        after(1).await;
        assert_eq!(
            service.list().len(),
            1,
            "skipped once: the second clip plays, one waits"
        );
        assert_ne!(session(&service), first);
    }

    #[tokio::test(start_paused = true)]
    async fn an_old_deadline_never_restarts_a_stopped_or_replayed_clip() {
        let (service, _) = service("stop", FakeLookup::new(None));
        service.add(KNOWN).await.unwrap();
        service.add(KNOWN).await.unwrap();
        after(5_000).await;
        service.stop();
        after(1).await;
        let stopped = service.playback();
        assert_eq!(stopped.state, State::Stopped);
        assert!(stopped.clip.is_some(), "the stopped clip stays for replay");
        after(60_000).await;
        assert_eq!(
            service.playback().state,
            State::Stopped,
            "the old deadline passed"
        );
        assert_eq!(service.list().len(), 1);

        let old = stopped.session_id.unwrap();
        service.replay(&old);
        after(1).await;
        let replayed = service.playback();
        assert_eq!(replayed.state, State::Loading);
        assert_ne!(replayed.session_id.as_deref(), Some(old.as_str()));
        assert_eq!(
            service.list().len(),
            1,
            "replay does not take from the queue"
        );
        service.replay(&old); // stale
        after(4_000).await;
        assert_eq!(service.playback().session_id, replayed.session_id);
        assert_eq!(service.playback().state, State::EstimatedPlaying);

        service.stop();
        service.play();
        after(1).await;
        assert_eq!(
            service.playback().state,
            State::Loading,
            "play takes the next one"
        );
        assert!(service.list().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_deleted_clip_errors_and_the_queue_moves_on() {
        let lookup = FakeLookup::new(None);
        let (service, _) = service("deleted", lookup.clone());
        service.add(KNOWN).await.unwrap();
        service.add(KNOWN).await.unwrap();
        after(1).await;
        lookup.fail(LookupError::NotFound); // deleted after it was queued
        service.skip(&session(&service));
        after(1).await;
        assert_eq!(service.playback().state, State::Error);
        after(2_000).await;
        assert_eq!(
            service.playback().state,
            State::Idle,
            "moved on after a pause"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_lookup_plays_the_queued_data() {
        let lookup = FakeLookup::new(None);
        let (service, _) = service("signed-out", lookup.clone());
        service.add(KNOWN).await.unwrap();
        service.add(KNOWN).await.unwrap();
        after(1).await;
        lookup.fail(LookupError::Twitch {
            status: 409,
            code: "twitch.sign_in_required".into(),
        });
        service.skip(&session(&service));
        after(3_001).await;
        let p = service.playback();
        assert_eq!(p.state, State::EstimatedPlaying);
        assert_eq!(p.clip.unwrap().duration_ms, 30_000);
    }

    #[tokio::test(start_paused = true)]
    async fn a_disabled_module_stops_and_starts_nothing() {
        let (service, _) = service("disabled", FakeLookup::new(None));
        service.add(KNOWN).await.unwrap();
        service.add(KNOWN).await.unwrap();
        after(5_000).await;
        service
            .0
            .modules
            .set_enabled(modules::CLIPS, false)
            .unwrap();
        service.stop(); // what the host does when the module is turned off
        after(1).await;
        assert_eq!(service.playback().state, State::Stopped);
        service.play();
        service.replay(&session(&service));
        after(60_000).await;
        assert_eq!(service.playback().state, State::Stopped);
        assert_eq!(service.list().len(), 1, "the queue is kept");

        service.0.modules.set_enabled(modules::CLIPS, true).unwrap();
        after(1).await;
        assert_eq!(
            service.playback().state,
            State::Stopped,
            "enabling starts nothing"
        );
    }
}
