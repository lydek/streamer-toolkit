//! Song requests (docs/modules/songs.md 點歌, 狀態轉換, 回覆): from chat (`!點歌`) or by hand, the queue and its
//! states, and the chat bot's replies. Storage is a port; replies and change notices go out through closures the
//! Host gives.

use std::{future::Future, path::PathBuf, sync::Arc};

use toolkit_core::{
    commands::{CommandTable, Declared},
    events::{ChatMessage, Source},
    modules::{self, Modules},
    settings::{self, SongReplies},
};

use super::{RepositoryError, SongRepository, new_id};
use crate::domain::requests::{
    Found, Outcome, QUEUE_MAX, ReplyVars, Request, RequestState, fill, find, open,
};
use toolkit_core::time::Timestamp;

/// The request command's key in the shared command table.
pub const COMMAND_KEY: &str = "songs.request";
pub const COMMANDS: &[Declared] = &[Declared {
    key: COMMAND_KEY,
    module: modules::SONGS,
    default_name: "點歌",
}];

/// Sung requests listed at most.
pub(crate) const SUNG_SHOWN: u32 = 50;
/// A viewer's words kept for `{輸入}`, in characters.
const WORDS_MAX_CHARS: usize = 100;
/// A name the streamer types for whom a song is, in characters.
pub(crate) const VIEWER_MAX_CHARS: usize = 25;

/// Posts a reply through the chat bot, which drops it while it is off.
pub type Say = Arc<dyn Fn(String) + Send + Sync>;
/// Tells the admin page the queue changed.
pub type Changed = Arc<dyn Fn() + Send + Sync>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum RequestError {
    #[error("songs.not_found")]
    NotFound,
    #[error("songs.already_queued")]
    AlreadyQueued,
    #[error("songs.queue_full")]
    QueueFull,
    /// The state table does not allow it (e.g. starting a song while another is sung).
    #[error("songs.not_allowed")]
    NotAllowed,
    #[error("songs.invalid")]
    Invalid,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

/// Storage port for requests.
pub trait RequestRepository: Send + Sync + 'static {
    /// Queued and singing requests, the queued ones in order.
    fn active_requests(&self)
    -> impl Future<Output = Result<Vec<Request>, RepositoryError>> + Send;
    /// Sung requests, the latest first, at most `limit`.
    fn sung_requests(
        &self,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<Request>, RepositoryError>> + Send;
    /// Adds a queued request and counts it on its song, in one transaction.
    fn add_request(&self, r: &Request) -> impl Future<Output = Result<(), RepositoryError>> + Send;
    /// Writes these requests' states, positions and times in order, in one transaction; one that became sung also
    /// sets its song's last sung time.
    fn save_requests(
        &self,
        rs: &[Request],
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send;
}

/// What the 點歌 page shows.
pub(crate) struct Board {
    pub(crate) singing: Option<Request>,
    pub(crate) queued: Vec<Request>,
    pub(crate) sung: Vec<Request>,
}

/// How a request names its song.
enum Pick<'a> {
    /// Picked on the page.
    Id(&'a str),
    /// A viewer's words in chat.
    Words(&'a str),
}

/// Who a request is for.
struct Viewer {
    id: Option<String>,
    name: Option<String>,
}

pub struct RequestsService<R> {
    repo: R,
    /// One change at a time, so the queue is checked against what is stored.
    lock: tokio::sync::Mutex<()>,
    modules: Arc<Modules>,
    commands: Arc<CommandTable>,
    data_dir: PathBuf,
    say: Say,
    changed: Changed,
}

impl<R: SongRepository + RequestRepository> RequestsService<R> {
    pub(crate) fn new(
        repo: R,
        modules: Arc<Modules>,
        commands: Arc<CommandTable>,
        data_dir: PathBuf,
        say: Say,
        changed: Changed,
    ) -> Self {
        Self {
            repo,
            lock: tokio::sync::Mutex::new(()),
            modules,
            commands,
            data_dir,
            say,
            changed,
        }
    }

    pub(crate) async fn board(&self) -> Result<Board, RepositoryError> {
        let active = self.repo.active_requests().await?;
        let (singing, queued) = active
            .into_iter()
            .partition::<Vec<_>, _>(|r| r.state == RequestState::Singing);
        Ok(Board {
            singing: singing.into_iter().next(),
            queued,
            sung: self.repo.sung_requests(SUNG_SHOWN).await?,
        })
    }

    /// Queues the song picked for `viewer` unless the queue rules say no (with `by_hand`, closed categories do not
    /// matter); the outcome, and the song's title and artist when one was found.
    async fn queue(
        &self,
        pick: Pick<'_>,
        viewer: Viewer,
        by_hand: bool,
    ) -> Result<(Outcome, Option<(String, String)>), RepositoryError> {
        let songs = self.repo.songs().await?;
        let found = match pick {
            Pick::Id(id) => songs
                .iter()
                .find(|s| s.id == id)
                .map_or(Found::None, Found::One),
            Pick::Words(words) => find(&songs, words),
        };
        let song = match found {
            Found::One(s) => s,
            Found::Several => return Ok((Outcome::Several, None)),
            Found::None => return Ok((Outcome::NotFound, None)),
        };
        let named = Some((song.title.clone(), song.artist.clone()));
        if !by_hand && !open(song, &self.repo.categories().await?) {
            return Ok((Outcome::NotOpen, named));
        }
        let active = self.repo.active_requests().await?;
        if active
            .iter()
            .any(|r| r.song_id.as_deref() == Some(&song.id))
        {
            return Ok((Outcome::AlreadyQueued, named));
        }
        let queued = active
            .iter()
            .filter(|r| r.state == RequestState::Queued)
            .count();
        if queued >= QUEUE_MAX {
            return Ok((Outcome::Full, named));
        }
        self.repo
            .add_request(&Request {
                id: new_id(),
                song_id: Some(song.id.clone()),
                title: song.title.clone(),
                artist: song.artist.clone(),
                viewer_id: viewer.id,
                viewer_name: viewer.name,
                by_hand,
                state: RequestState::Queued,
                position: active.iter().map(|r| r.position).max().unwrap_or(0) + 1,
                requested_at: Timestamp::now(),
                finished_at: None,
            })
            .await?;
        Ok((Outcome::Queued(queued + 1), named))
    }

    /// A chat message from Twitch: a request when it is the request command and its sender may use it. Simulated
    /// chat, and chat while the module is off, never queue; the Host leaves out the chat bot's own messages.
    pub fn receive_chat(self: &Arc<Self>, m: &ChatMessage, source: Source) {
        if source != Source::Twitch || !self.modules.is_enabled(modules::SONGS) {
            return;
        }
        let entry = self.commands.get(COMMAND_KEY);
        if !entry.matches(&m.text) || !entry.allowed.permits(m) {
            return;
        }
        let words: String = m
            .text
            .trim()
            .split_once(char::is_whitespace)
            .map_or("", |(_, w)| w.trim())
            .chars()
            .take(WORDS_MAX_CHARS)
            .collect();
        if words.is_empty() {
            return;
        }
        let me = self.clone();
        let viewer = Viewer {
            id: Some(m.user_id.clone()),
            name: Some(m.user_name.clone()),
        };
        tokio::spawn(async move {
            if let Err(e) = me.answer_chat(&words, viewer).await {
                tracing::warn!(code = "songs.storage_failed", error = %e, "song request failed");
            }
        });
    }

    async fn answer_chat(&self, words: &str, viewer: Viewer) -> Result<(), RepositoryError> {
        let name = viewer.name.clone().unwrap_or_default();
        let (outcome, named) = {
            let _one = self.lock.lock().await;
            self.queue(Pick::Words(words), viewer, false).await?
        };
        let (title, artist) = named.unwrap_or_default();
        let replies = settings::load(&self.data_dir).songs.replies;
        let vars = ReplyVars {
            viewer: &name,
            title: &title,
            artist: &artist,
            position: match outcome {
                Outcome::Queued(p) => Some(p),
                _ => None,
            },
            words,
        };
        (self.say)(fill(reply(&replies, outcome), &vars));
        if matches!(outcome, Outcome::Queued(_)) {
            (self.changed)();
        }
        Ok(())
    }

    /// The streamer queues `song_id` on the page, for `viewer` (a typed name) or nobody; closed categories do not
    /// matter. Nothing is posted in chat.
    pub(crate) async fn request_by_hand(
        &self,
        song_id: &str,
        viewer: Option<String>,
    ) -> Result<(), RequestError> {
        let viewer = viewer
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty());
        if viewer
            .as_ref()
            .is_some_and(|v| v.chars().count() > VIEWER_MAX_CHARS)
        {
            return Err(RequestError::Invalid);
        }
        let _one = self.lock.lock().await;
        let viewer = Viewer {
            id: None,
            name: viewer,
        };
        match self.queue(Pick::Id(song_id), viewer, true).await?.0 {
            Outcome::Queued(_) => {}
            Outcome::NotFound | Outcome::Several => return Err(RequestError::NotFound),
            Outcome::AlreadyQueued => return Err(RequestError::AlreadyQueued),
            Outcome::Full => return Err(RequestError::QueueFull),
            Outcome::NotOpen => unreachable!("by hand ignores closed categories"),
        }
        (self.changed)();
        Ok(())
    }

    /// Saves `changed` requests and tells the page.
    async fn save(&self, changed: &[Request]) -> Result<(), RequestError> {
        self.repo.save_requests(changed).await?;
        (self.changed)();
        Ok(())
    }

    /// Starts singing queued request `id`; only while nothing is being sung.
    pub(crate) async fn start(&self, id: &str) -> Result<(), RequestError> {
        let _one = self.lock.lock().await;
        let active = self.repo.active_requests().await?;
        let mut r = active
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or(RequestError::NotFound)?;
        if active.iter().any(|r| r.state == RequestState::Singing)
            || !r.move_to(RequestState::Singing, Timestamp::now())
        {
            return Err(RequestError::NotAllowed);
        }
        self.save(&[r]).await
    }

    /// 唱完了: the song being sung is sung, and the first queued one starts.
    pub(crate) async fn finish(&self) -> Result<(), RequestError> {
        let _one = self.lock.lock().await;
        let active = self.repo.active_requests().await?;
        let now = Timestamp::now();
        let mut sung = active
            .iter()
            .find(|r| r.state == RequestState::Singing)
            .cloned()
            .ok_or(RequestError::NotAllowed)?;
        sung.move_to(RequestState::Sung, now);
        let mut changed = vec![sung];
        if let Some(mut next) = active.into_iter().find(|r| r.state == RequestState::Queued) {
            next.move_to(RequestState::Singing, now);
            changed.push(next);
        }
        self.save(&changed).await
    }

    /// Removes queued or singing request `id`; the rest keep their order.
    pub(crate) async fn remove(&self, id: &str) -> Result<(), RequestError> {
        let _one = self.lock.lock().await;
        let mut r = self
            .repo
            .active_requests()
            .await?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or(RequestError::NotFound)?;
        r.move_to(RequestState::Removed, Timestamp::now());
        self.save(&[r]).await
    }

    /// Puts the queued requests in the order of `ids`, which must name each of them once.
    pub(crate) async fn reorder(&self, ids: &[String]) -> Result<(), RequestError> {
        let _one = self.lock.lock().await;
        let queued: Vec<Request> = self
            .repo
            .active_requests()
            .await?
            .into_iter()
            .filter(|r| r.state == RequestState::Queued)
            .collect();
        let mut sorted = ids.to_vec();
        sorted.sort();
        sorted.dedup();
        if sorted.len() != ids.len() || ids.len() != queued.len() {
            return Err(RequestError::Invalid);
        }
        let mut changed = Vec::with_capacity(ids.len());
        for (i, id) in ids.iter().enumerate() {
            let r = queued
                .iter()
                .find(|r| &r.id == id)
                .ok_or(RequestError::Invalid)?;
            changed.push(Request {
                position: i as i64 + 1,
                ..r.clone()
            });
        }
        self.save(&changed).await
    }
}

fn reply(replies: &SongReplies, outcome: Outcome) -> &str {
    match outcome {
        Outcome::Queued(_) => &replies.queued,
        Outcome::NotFound => &replies.not_found,
        Outcome::Several => &replies.several,
        Outcome::NotOpen => &replies.not_open,
        Outcome::AlreadyQueued => &replies.already_queued,
        Outcome::Full => &replies.full,
    }
}
