//! One queue for what the overlays present (alerts, stamp cards, reward sounds and videos): items play one at a time, in the
//! order they came, with no size limit, so nothing overlaps and nothing is dropped. Modules still decide what to
//! show; the queue only paces it. The broadcaster can skip the item playing, clear what waits, or stop everything
//! at once.

use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, watch};
use utoipa::ToSchema;

/// The pause between two items.
pub const DEFAULT_GAP: Duration = Duration::from_secs(1);

/// The overlay that presents an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Alerts,
    Checkin,
    Sound,
}

/// What an item is, as the queue area names it; names are raw text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Subject {
    /// An alert; `alert` is its kind as in the `alert` payload (`follow`, `subscribe`, ...). No viewer for an
    /// anonymous gift or cheer.
    Alert {
        alert: String,
        viewer: Option<String>,
    },
    /// A stamp card with `stamps` of its `slots` filled.
    Card {
        viewer: String,
        stamps: u32,
        slots: u32,
    },
    /// A reward sound and the viewer who redeemed it.
    Sound { sound: String, viewer: String },
    /// A reward's video or GIF and the viewer who redeemed it.
    Media { media: String, viewer: String },
}

/// Something the Host itself can see went wrong with an item (overlays do not report back).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Problem {
    /// Its sound file is gone from the sounds folder; the rest still shows.
    SoundMissing,
}

/// What `present` did with an item when its turn came.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    /// Skipped after all (e.g. its module was turned off while it waited); it costs no time.
    No,
    Yes,
    /// Shown, but its sound file is missing.
    WithoutSound,
}

/// What finally became of an item, as far as the Host can tell (overlays do not report back).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Shown for its whole time.
    Played,
    /// Shown for its whole time, without its sound: the file is missing.
    SoundMissing,
    /// Its overlay was not open in OBS when its turn came.
    OffAir,
    /// Its module turned it down when its turn came (e.g. turned off meanwhile).
    Declined,
    /// Ended by 跳過.
    Skipped,
    /// Dropped by 清空排隊 before its turn.
    Cleared,
    /// Ended or dropped by the emergency stop.
    Stopped,
}

pub struct Item {
    pub stage: Stage,
    /// What the queue area shows for it.
    pub subject: Subject,
    /// How long the overlay shows it.
    pub show: Duration,
    /// From the simulator: shown even when the overlay is not open in OBS, so the admin window's preview still
    /// gets it.
    pub simulated: bool,
    /// Work it needs before it can show (a stamp card's avatar lookup); it holds its place meanwhile, and a stop
    /// ends it.
    pub ready: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
    /// Publishes it to the overlay.
    pub present: Box<dyn FnOnce() -> Shown + Send>,
    /// Told what finally became of it; called outside the queue's lock.
    pub done: Option<Box<dyn FnOnce(Outcome) + Send>>,
}

fn report(done: Option<Box<dyn FnOnce(Outcome) + Send>>, outcome: Outcome) {
    if let Some(done) = done {
        done(outcome);
    }
}

/// What the queue holds right now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueueStatus {
    /// The item on its overlay now; `null` while nothing plays.
    #[schema(required)]
    pub current: Option<Playing>,
    /// In the order they will play.
    pub waiting: Vec<Waiting>,
    /// The pause after each item, in ms.
    pub gap_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Playing {
    pub stage: Stage,
    pub subject: Subject,
    pub show_ms: u64,
    /// RFC 3339 time it started.
    pub started_at: String,
    /// What went wrong that the Host can see; `null` when nothing did.
    #[schema(required)]
    pub problem: Option<Problem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Waiting {
    pub stage: Stage,
    pub subject: Subject,
    pub show_ms: u64,
    /// Its overlay is open in OBS right now; when it is not as its turn comes, the item is skipped.
    pub on_air: bool,
}

type OnAir = Box<dyn Fn(Stage) -> bool + Send + Sync>;

#[derive(Default)]
struct State {
    waiting: VecDeque<Item>,
    current: Option<Playing>,
}

pub struct Presenter {
    state: Mutex<State>,
    ready: Notify,
    gap: Duration,
    on_air: OnAir,
    status: watch::Sender<QueueStatus>,
    /// Counts emergency stops; the item playing ends its wait when it changes.
    stops: watch::Sender<u64>,
    /// Counts skips; the item playing when it changes ends, and the next one follows after the gap.
    skips: watch::Sender<u64>,
}

impl Presenter {
    /// Starts the player; `on_air` says whether a stage's overlay is open in OBS right now. Needs a Tokio runtime.
    pub fn start(
        gap: Duration,
        on_air: impl Fn(Stage) -> bool + Send + Sync + 'static,
    ) -> Arc<Self> {
        let presenter = Arc::new(Self {
            state: Mutex::default(),
            ready: Notify::new(),
            gap,
            on_air: Box::new(on_air),
            status: watch::channel(QueueStatus {
                gap_ms: millis(gap),
                ..QueueStatus::default()
            })
            .0,
            stops: watch::channel(0).0,
            skips: watch::channel(0).0,
        });
        tokio::spawn(play(presenter.clone()));
        presenter
    }

    /// Queues `item` behind everything already waiting.
    pub fn push(&self, item: Item) {
        let mut state = self.state.lock().unwrap();
        state.waiting.push_back(item);
        self.publish(&state);
        drop(state);
        self.ready.notify_one();
    }

    /// Follows the queue: the current status, then every change.
    pub fn subscribe(&self) -> watch::Receiver<QueueStatus> {
        self.status.subscribe()
    }

    /// Drops every waiting item; the one playing finishes. Returns how many were dropped.
    pub fn clear(&self) -> usize {
        let mut state = self.state.lock().unwrap();
        let dropped: Vec<Item> = state.waiting.drain(..).collect();
        self.publish(&state);
        drop(state);
        let cancelled = dropped.len();
        for item in dropped {
            report(item.done, Outcome::Cleared);
        }
        cancelled
    }

    /// Ends the item playing now; the waiting ones follow after the gap. Returns whether one was playing.
    ///
    /// `announce` tells the overlays which stage to stop (gets the item's stage) while the queue is still locked.
    pub fn skip(&self, announce: impl FnOnce(Stage)) -> bool {
        let mut state = self.state.lock().unwrap();
        let Some(playing) = state.current.take() else {
            return false;
        };
        self.publish(&state);
        self.skips.send_modify(|n| *n += 1);
        announce(playing.stage);
        true
    }

    /// Publishes the status again, e.g. after an overlay opened or closed in OBS.
    pub fn refresh(&self) {
        let state = self.state.lock().unwrap();
        self.publish(&state);
    }

    /// Drops every waiting item and ends the one playing now; the caller tells the overlays to stop at once.
    /// Returns whether one was playing and how many waiting ones were dropped.
    ///
    /// `announce` tells the overlays (gets the same two values) while the queue is still locked, so nothing taken
    /// after the stop can reach an overlay before the stop does.
    pub fn stop(&self, announce: impl FnOnce(bool, usize)) -> (bool, usize) {
        let mut state = self.state.lock().unwrap();
        let dropped: Vec<Item> = state.waiting.drain(..).collect();
        let cancelled = dropped.len();
        let playing = state.current.take().is_some();
        self.publish(&state);
        self.stops.send_modify(|n| *n += 1);
        announce(playing, cancelled);
        drop(state);
        for item in dropped {
            report(item.done, Outcome::Stopped);
        }
        (playing, cancelled)
    }

    fn publish(&self, state: &State) {
        self.status.send_replace(QueueStatus {
            current: state.current.clone(),
            waiting: state
                .waiting
                .iter()
                .map(|i| Waiting {
                    stage: i.stage,
                    subject: i.subject.clone(),
                    show_ms: millis(i.show),
                    on_air: i.simulated || (self.on_air)(i.stage),
                })
                .collect(),
            gap_ms: millis(self.gap),
        });
    }
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// Presents the waiting items in order, each after the previous one has shown. An item whose overlay is not open
/// is skipped without waiting: nobody would see it. An emergency stop ends the wait of the one playing; a skip
/// ends it and keeps the gap before the next.
async fn play(presenter: Arc<Presenter>) {
    loop {
        // Taken, checked and marked as playing in one step under the lock stop() takes, with the stop signal read
        // there too: a stop after this ends the item, a stop before it never sees it.
        let next = {
            let mut state = presenter.state.lock().unwrap();
            match state.waiting.pop_front() {
                None => None,
                Some(item) => {
                    let taken = if item.simulated || (presenter.on_air)(item.stage) {
                        state.current = Some(Playing {
                            stage: item.stage,
                            subject: item.subject.clone(),
                            show_ms: millis(item.show),
                            started_at: chrono::Utc::now()
                                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                            problem: None,
                        });
                        Ok((
                            item,
                            presenter.stops.subscribe(),
                            presenter.skips.subscribe(),
                        ))
                    } else {
                        tracing::debug!(stage = ?item.stage, "overlay not open, item skipped");
                        Err(item.done)
                    };
                    presenter.publish(&state);
                    Some(taken)
                }
            }
        };
        let (item, mut stops, mut skips) = match next {
            Some(Ok(taken)) => taken,
            Some(Err(done)) => {
                report(done, Outcome::OffAir);
                continue;
            }
            None => {
                // A notify sent since the queue was found empty is kept, so nothing waits forever.
                presenter.ready.notified().await;
                continue;
            }
        };
        let Item {
            show,
            ready,
            present,
            done,
            ..
        } = item;
        let mut ended = None;
        if let Some(ready) = ready {
            tokio::select! {
                _ = ready => {}
                _ = stops.changed() => ended = Some(Outcome::Stopped),
                _ = skips.changed() => ended = Some(Outcome::Skipped),
            }
        }
        // Checked and published under the lock stop() and skip() take, so neither is ever followed by this item's
        // picture.
        let shown = {
            let mut state = presenter.state.lock().unwrap();
            if ended.is_none() && stops.has_changed().unwrap_or(false) {
                ended = Some(Outcome::Stopped);
            }
            if ended.is_none() && skips.has_changed().unwrap_or(false) {
                ended = Some(Outcome::Skipped);
            }
            let shown = if ended.is_some() {
                Shown::No
            } else {
                present()
            };
            if shown == Shown::WithoutSound
                && let Some(playing) = &mut state.current
            {
                playing.problem = Some(Problem::SoundMissing);
                presenter.publish(&state);
            }
            shown
        };
        let outcome = match (ended, shown) {
            (Some(outcome), _) => outcome,
            (None, Shown::No) => Outcome::Declined,
            (None, shown) => {
                let full = match shown {
                    Shown::WithoutSound => Outcome::SoundMissing,
                    _ => Outcome::Played,
                };
                tokio::select! {
                    _ = tokio::time::sleep(show + presenter.gap) => full,
                    _ = stops.changed() => Outcome::Stopped,
                    _ = skips.changed() => Outcome::Skipped,
                }
            }
        };
        // A skip keeps the gap before the next item, whether it came while playing or while getting ready.
        if outcome == Outcome::Skipped {
            tokio::select! {
                _ = tokio::time::sleep(presenter.gap) => {}
                _ = stops.changed() => {}
            }
        }
        {
            let mut state = presenter.state.lock().unwrap();
            state.current = None;
            presenter.publish(&state);
        }
        report(done, outcome);
    }
}

/// One item a module queued, and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentItem {
    /// Unique in this launch.
    pub id: u64,
    /// What it was queued for, in its module's terms (e.g. a redemption's Twitch ID); `None` when nothing is
    /// recorded for it (a simulation).
    pub key: Option<String>,
    pub subject: Subject,
    pub source: crate::events::Source,
    /// RFC 3339, UTC.
    pub queued_at: String,
    /// `None` while it waits or plays.
    pub outcome: Option<Outcome>,
}

/// What became of the latest items one module queued (in memory, this launch only), newest first.
#[derive(Clone, Default)]
pub struct Recent {
    items: Arc<Mutex<(u64, VecDeque<RecentItem>)>>,
}

impl Recent {
    /// How many it keeps.
    pub const KEEP: usize = 50;

    /// Records an item as queued; give the returned callback to its [`Item::done`] to record its outcome.
    pub fn record(
        &self,
        key: Option<String>,
        subject: Subject,
        source: crate::events::Source,
    ) -> Box<dyn FnOnce(Outcome) + Send> {
        let id = {
            let mut items = self.items.lock().unwrap();
            items.0 += 1;
            let id = items.0;
            items.1.push_front(RecentItem {
                id,
                key,
                subject,
                source,
                queued_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                outcome: None,
            });
            items.1.truncate(Self::KEEP);
            id
        };
        let items = self.items.clone();
        Box::new(move |outcome| {
            // An item pushed out of the list meanwhile is simply not found.
            if let Some(r) = items.lock().unwrap().1.iter_mut().find(|r| r.id == id) {
                r.outcome = Some(outcome);
            }
        })
    }

    /// Newest first.
    pub fn list(&self) -> Vec<RecentItem> {
        self.items.lock().unwrap().1.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::{sync::mpsc, time::Instant};

    fn item(
        stage: Stage,
        show_ms: u64,
        tx: &mpsc::UnboundedSender<(u32, Instant)>,
        n: u32,
    ) -> Item {
        let tx = tx.clone();
        Item {
            stage,
            show: Duration::from_millis(show_ms),
            subject: Subject::Sound {
                sound: format!("item {n}"),
                viewer: "viewer".into(),
            },
            simulated: false,
            ready: None,
            present: Box::new(move || match tx.send((n, Instant::now())) {
                Ok(()) => Shown::Yes,
                Err(_) => Shown::No,
            }),
            done: None,
        }
    }

    /// An item for `stage` that shows as `shown` says, with its outcome recorded in `recent`.
    fn recorded(stage: Stage, shown: Shown, recent: &Recent, n: u32) -> Item {
        let subject = Subject::Sound {
            sound: format!("item {n}"),
            viewer: "viewer".into(),
        };
        Item {
            stage,
            show: Duration::from_millis(1000),
            done: Some(recent.record(
                Some(format!("k{n}")),
                subject.clone(),
                crate::events::Source::Twitch,
            )),
            subject,
            simulated: false,
            ready: None,
            present: Box::new(move || shown),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn recent_items_record_every_outcome_and_keep_fifty() {
        let open = Arc::new(AtomicBool::new(true));
        let on_air = open.clone();
        let presenter = Presenter::start(Duration::from_millis(100), move |_| {
            on_air.load(Ordering::Relaxed)
        });
        let recent = Recent::default();
        let outcome_of = |n: u32| {
            recent
                .list()
                .into_iter()
                .find(|r| {
                    r.subject
                        == Subject::Sound {
                            sound: format!("item {n}"),
                            viewer: "viewer".into(),
                        }
                })
                .unwrap()
                .outcome
        };
        presenter.push(recorded(Stage::Sound, Shown::Yes, &recent, 1));
        presenter.push(recorded(Stage::Sound, Shown::WithoutSound, &recent, 2));
        presenter.push(recorded(Stage::Sound, Shown::No, &recent, 3));
        assert_eq!(outcome_of(1), None, "queued");
        tokio::time::sleep(Duration::from_millis(3500)).await;
        assert_eq!(
            [outcome_of(1), outcome_of(2), outcome_of(3)],
            [
                Some(Outcome::Played),
                Some(Outcome::SoundMissing),
                Some(Outcome::Declined)
            ]
        );

        open.store(false, Ordering::Relaxed);
        presenter.push(recorded(Stage::Sound, Shown::Yes, &recent, 4));
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(outcome_of(4), Some(Outcome::OffAir));
        open.store(true, Ordering::Relaxed);

        presenter.push(recorded(Stage::Sound, Shown::Yes, &recent, 5));
        presenter.push(recorded(Stage::Sound, Shown::Yes, &recent, 6));
        tokio::time::sleep(Duration::from_millis(200)).await;
        presenter.skip(|_| {});
        presenter.clear();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            [outcome_of(5), outcome_of(6)],
            [Some(Outcome::Skipped), Some(Outcome::Cleared)]
        );

        presenter.push(recorded(Stage::Sound, Shown::Yes, &recent, 7));
        presenter.push(recorded(Stage::Sound, Shown::Yes, &recent, 8));
        tokio::time::sleep(Duration::from_millis(200)).await;
        presenter.stop(|_, _| {});
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            [outcome_of(7), outcome_of(8)],
            [Some(Outcome::Stopped), Some(Outcome::Stopped)]
        );

        for n in 100..160 {
            let _ = recent.record(
                None,
                Subject::Sound {
                    sound: format!("item {n}"),
                    viewer: "v".into(),
                },
                crate::events::Source::Twitch,
            );
        }
        let all = recent.list();
        assert_eq!(all.len(), Recent::KEEP);
        assert_eq!(
            all[0].subject,
            Subject::Sound {
                sound: "item 159".into(),
                viewer: "v".into()
            },
            "newest first"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn plays_in_order_one_at_a_time_without_a_limit() {
        let presenter = Presenter::start(Duration::from_millis(100), |_| true);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let start = Instant::now();
        for n in 0..300 {
            let stage = if n % 2 == 0 {
                Stage::Alerts
            } else {
                Stage::Checkin
            };
            presenter.push(item(stage, 1000, &tx, n));
        }
        for n in 0..300 {
            let (got, at) = rx.recv().await.unwrap();
            assert_eq!(got, n, "in arrival order, none dropped");
            // Each waits for the one before: show time plus the gap.
            assert_eq!(at - start, Duration::from_millis(1100) * n);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn an_overlay_that_is_not_open_is_skipped_without_waiting() {
        let alerts_open = Arc::new(AtomicBool::new(false));
        let open = alerts_open.clone();
        let presenter = Presenter::start(Duration::from_millis(100), move |stage| {
            stage != Stage::Alerts || open.load(Ordering::Relaxed)
        });
        let (tx, mut rx) = mpsc::unbounded_channel();
        let start = Instant::now();
        presenter.push(item(Stage::Alerts, 1000, &tx, 0));
        presenter.push(item(Stage::Checkin, 1000, &tx, 1));
        assert_eq!(
            rx.recv().await.unwrap(),
            (1, start),
            "the alert is skipped and costs no time"
        );

        // A simulated one still shows: the admin window's preview gets it without OBS.
        presenter.push(Item {
            simulated: true,
            ..item(Stage::Alerts, 1000, &tx, 9)
        });
        let (n, at) = rx.recv().await.unwrap();
        assert_eq!((n, at - start), (9, Duration::from_millis(1100)));
        let start = at;

        alerts_open.store(true, Ordering::Relaxed);
        presenter.push(item(Stage::Alerts, 1000, &tx, 2));
        let (n, at) = rx.recv().await.unwrap();
        assert_eq!((n, at - start), (2, Duration::from_millis(1100)));
    }

    #[tokio::test(start_paused = true)]
    async fn an_item_that_declines_costs_no_time() {
        let presenter = Presenter::start(Duration::from_millis(100), |_| true);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let start = Instant::now();
        presenter.push(Item {
            stage: Stage::Alerts,
            show: Duration::from_secs(5),
            subject: Subject::Sound {
                sound: "declines".into(),
                viewer: "v".into(),
            },
            simulated: false,
            ready: None,
            present: Box::new(|| Shown::No),
            done: None,
        });
        presenter.push(item(Stage::Alerts, 1000, &tx, 1));
        assert_eq!(rx.recv().await.unwrap(), (1, start));
    }

    #[tokio::test(start_paused = true)]
    async fn the_status_follows_what_plays_and_what_waits() {
        let presenter = Presenter::start(Duration::from_millis(100), |_| true);
        let mut status = presenter.subscribe();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let fan = Subject::Alert {
            alert: "follow".into(),
            viewer: Some("Fan".into()),
        };
        let bell = Subject::Sound {
            sound: "Bell".into(),
            viewer: "Kai".into(),
        };
        presenter.push(Item {
            subject: fan.clone(),
            ..item(Stage::Alerts, 1000, &tx, 0)
        });
        presenter.push(Item {
            subject: bell.clone(),
            ..item(Stage::Sound, 2000, &tx, 1)
        });
        rx.recv().await.unwrap();
        let now = status.borrow_and_update().clone();
        let current = now.current.unwrap();
        assert_eq!(
            (
                current.stage,
                current.subject,
                current.show_ms,
                current.problem
            ),
            (Stage::Alerts, fan, 1000, None)
        );
        assert_eq!(
            now.waiting,
            [Waiting {
                stage: Stage::Sound,
                subject: bell,
                show_ms: 2000,
                on_air: true,
            }]
        );
        assert_eq!(now.gap_ms, 100);

        rx.recv().await.unwrap();
        tokio::time::sleep(Duration::from_millis(2200)).await;
        assert_eq!(
            *status.borrow_and_update(),
            QueueStatus {
                current: None,
                waiting: vec![],
                gap_ms: 100
            },
            "empty once both have played"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn clear_drops_what_waits_and_the_one_playing_finishes() {
        let presenter = Presenter::start(Duration::from_millis(100), |_| true);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let start = Instant::now();
        for n in 0..3 {
            presenter.push(item(Stage::Alerts, 1000, &tx, n));
        }
        rx.recv().await.unwrap();
        assert_eq!(presenter.clear(), 2);
        presenter.push(item(Stage::Alerts, 1000, &tx, 9));
        let (n, at) = rx.recv().await.unwrap();
        assert_eq!(
            (n, at - start),
            (9, Duration::from_millis(1100)),
            "the cleared ones never play; the one playing keeps its time"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_item_getting_ready_keeps_its_place_and_a_stop_ends_it() {
        let presenter = Presenter::start(Duration::from_millis(100), |_| true);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let start = Instant::now();
        presenter.push(Item {
            ready: Some(Box::pin(tokio::time::sleep(Duration::from_millis(500)))),
            ..item(Stage::Checkin, 1000, &tx, 0)
        });
        presenter.push(item(Stage::Alerts, 1000, &tx, 1));
        let (n, at) = rx.recv().await.unwrap();
        assert_eq!(
            (n, at - start),
            (0, Duration::from_millis(500)),
            "the alert waited behind the card"
        );
        assert_eq!(rx.recv().await.unwrap().0, 1);

        presenter.push(Item {
            ready: Some(Box::pin(tokio::time::sleep(Duration::from_secs(3)))),
            ..item(Stage::Checkin, 1000, &tx, 2)
        });
        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert_eq!(
            presenter.stop(|_, _| {}),
            (true, 0),
            "the card getting ready counts as playing"
        );
        presenter.push(item(Stage::Alerts, 1000, &tx, 3));
        assert_eq!(
            rx.recv().await.unwrap().0,
            3,
            "the stopped card never shows"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_item_queued_after_a_stop_plays() {
        let presenter = Presenter::start(Duration::from_millis(100), |_| true);
        let (tx, mut rx) = mpsc::unbounded_channel();
        // The player is idle (waiting for work) when the stop comes.
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(presenter.stop(|_, _| {}), (false, 0));
        presenter.push(item(Stage::Alerts, 1000, &tx, 1));
        assert_eq!(rx.recv().await.unwrap().0, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn the_stop_is_announced_before_anything_after_it_can_play() {
        let presenter = Presenter::start(Duration::from_millis(100), |_| true);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let log = Arc::new(Mutex::new(Vec::new()));
        presenter.push(item(Stage::Alerts, 5000, &tx, 0));
        rx.recv().await.unwrap();
        let (stop_log, play_log) = (log.clone(), log.clone());
        presenter.stop(move |_, _| stop_log.lock().unwrap().push("stopped"));
        let tx2 = tx.clone();
        presenter.push(Item {
            present: Box::new(move || {
                play_log.lock().unwrap().push("played");
                tx2.send((1, Instant::now())).unwrap();
                Shown::Yes
            }),
            ..item(Stage::Alerts, 1000, &tx, 1)
        });
        rx.recv().await.unwrap();
        assert_eq!(*log.lock().unwrap(), ["stopped", "played"]);
    }

    #[tokio::test(start_paused = true)]
    async fn stop_ends_the_one_playing_and_drops_what_waits() {
        let presenter = Presenter::start(Duration::from_millis(100), |_| true);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let start = Instant::now();
        for n in 0..3 {
            presenter.push(item(Stage::Alerts, 5000, &tx, n));
        }
        rx.recv().await.unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(presenter.stop(|_, _| {}), (true, 2));
        assert_eq!(presenter.subscribe().borrow().current, None);
        presenter.push(item(Stage::Alerts, 1000, &tx, 9));
        let (n, at) = rx.recv().await.unwrap();
        assert_eq!(
            (n, at - start),
            (9, Duration::from_millis(500)),
            "the next item plays at once, without the stopped one's wait"
        );
        assert_eq!(presenter.stop(|_, _| {}), (true, 0));
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(
            presenter.stop(|_, _| {}),
            (false, 0),
            "nothing left to stop"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn skip_ends_the_one_playing_and_the_next_follows_after_the_gap() {
        let presenter = Presenter::start(Duration::from_millis(100), |_| true);
        let (tx, mut rx) = mpsc::unbounded_channel();
        presenter.push(item(Stage::Alerts, 5000, &tx, 0));
        presenter.push(item(Stage::Sound, 1000, &tx, 1));
        let (_, started) = rx.recv().await.unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        let announced = Arc::new(Mutex::new(None));
        let log = announced.clone();
        assert!(presenter.skip(move |stage| *log.lock().unwrap() = Some(stage)));
        assert_eq!(*announced.lock().unwrap(), Some(Stage::Alerts));
        assert_eq!(presenter.subscribe().borrow().current, None);
        assert_eq!(
            presenter.subscribe().borrow().waiting.len(),
            1,
            "what waits stays"
        );
        let (n, at) = rx.recv().await.unwrap();
        assert_eq!(n, 1);
        assert_eq!(
            at - started,
            Duration::from_millis(600),
            "skipped at 500 ms, then the gap"
        );
        // Nothing playing: nothing to skip, nothing announced.
        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert!(!presenter.skip(|_| panic!("nothing to announce")));
    }

    #[tokio::test(start_paused = true)]
    async fn a_skip_while_getting_ready_ends_it_unseen() {
        let presenter = Presenter::start(Duration::from_millis(100), |_| true);
        let (tx, mut rx) = mpsc::unbounded_channel();
        presenter.push(Item {
            ready: Some(Box::pin(std::future::pending())),
            ..item(Stage::Checkin, 5000, &tx, 0)
        });
        presenter.push(item(Stage::Alerts, 1000, &tx, 1));
        tokio::time::sleep(Duration::from_millis(10)).await;
        let skipped = Instant::now();
        assert!(presenter.skip(|_| {}));
        let (n, at) = rx.recv().await.unwrap();
        assert_eq!(n, 1, "the card never showed");
        assert_eq!(
            at - skipped,
            Duration::from_millis(100),
            "the gap still comes first"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_missing_sound_file_marks_the_item_and_it_still_shows() {
        let presenter = Presenter::start(Duration::from_millis(100), |_| true);
        let mut status = presenter.subscribe();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let tx2 = tx.clone();
        presenter.push(Item {
            present: Box::new(move || {
                tx2.send((0, Instant::now())).unwrap();
                Shown::WithoutSound
            }),
            ..item(Stage::Alerts, 1000, &tx, 0)
        });
        let (_, started) = rx.recv().await.unwrap();
        status
            .wait_for(|s| s.current.as_ref().is_some_and(|c| c.problem.is_some()))
            .await
            .unwrap();
        presenter.push(item(Stage::Alerts, 1000, &tx, 1));
        let (_, next) = rx.recv().await.unwrap();
        assert_eq!(
            next - started,
            Duration::from_millis(1100),
            "it kept its time"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn waiting_items_say_whether_their_overlay_is_open_and_refresh_follows_obs() {
        let open = Arc::new(AtomicBool::new(false));
        let on_air = open.clone();
        let presenter = Presenter::start(Duration::from_millis(100), move |stage| {
            stage == Stage::Alerts || on_air.load(Ordering::Relaxed)
        });
        let (tx, mut rx) = mpsc::unbounded_channel();
        presenter.push(item(Stage::Alerts, 5000, &tx, 0));
        rx.recv().await.unwrap();
        presenter.push(item(Stage::Sound, 1000, &tx, 1));
        presenter.push(Item {
            simulated: true,
            ..item(Stage::Sound, 1000, &tx, 2)
        });
        let on_air = |p: &Presenter| -> Vec<bool> {
            p.subscribe()
                .borrow()
                .waiting
                .iter()
                .map(|w| w.on_air)
                .collect()
        };
        assert_eq!(
            on_air(&presenter),
            [false, true],
            "simulated items always show"
        );
        open.store(true, Ordering::Relaxed);
        presenter.refresh();
        assert_eq!(on_air(&presenter), [true, true]);
    }

    #[tokio::test(start_paused = true)]
    async fn each_item_is_told_what_became_of_it() {
        let open = Arc::new(AtomicBool::new(true));
        let on_air = open.clone();
        let presenter = Presenter::start(Duration::from_millis(100), move |stage| {
            stage != Stage::Checkin || on_air.load(Ordering::Relaxed)
        });
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (out_tx, mut outcomes) = mpsc::unbounded_channel();
        let tracked = |item: Item, n: u32| {
            let out = out_tx.clone();
            Item {
                done: Some(Box::new(move |o| out.send((n, o)).unwrap())),
                ..item
            }
        };
        open.store(false, Ordering::Relaxed);
        presenter.push(tracked(item(Stage::Checkin, 1000, &tx, 0), 0));
        presenter.push(tracked(item(Stage::Alerts, 1000, &tx, 1), 1));
        presenter.push(tracked(
            Item {
                present: Box::new(|| Shown::WithoutSound),
                ..item(Stage::Alerts, 1000, &tx, 2)
            },
            2,
        ));
        presenter.push(tracked(
            Item {
                present: Box::new(|| Shown::No),
                ..item(Stage::Alerts, 1000, &tx, 3)
            },
            3,
        ));
        presenter.push(tracked(item(Stage::Alerts, 5000, &tx, 4), 4));
        assert_eq!(outcomes.recv().await.unwrap(), (0, Outcome::OffAir));
        assert_eq!(outcomes.recv().await.unwrap(), (1, Outcome::Played));
        assert_eq!(outcomes.recv().await.unwrap(), (2, Outcome::SoundMissing));
        assert_eq!(outcomes.recv().await.unwrap(), (3, Outcome::Declined));
        rx.recv().await.unwrap();
        rx.recv().await.unwrap();
        presenter.push(tracked(item(Stage::Alerts, 1000, &tx, 5), 5));
        presenter.push(tracked(item(Stage::Alerts, 1000, &tx, 6), 6));
        assert!(presenter.skip(|_| {}));
        assert_eq!(outcomes.recv().await.unwrap(), (4, Outcome::Skipped));
        rx.recv().await.unwrap();
        presenter.clear();
        assert_eq!(outcomes.recv().await.unwrap(), (6, Outcome::Cleared));
        presenter.push(tracked(item(Stage::Alerts, 1000, &tx, 7), 7));
        presenter.stop(|_, _| {});
        let mut rest = vec![
            outcomes.recv().await.unwrap(),
            outcomes.recv().await.unwrap(),
        ];
        rest.sort_by_key(|(n, _)| *n);
        assert_eq!(rest, [(5, Outcome::Stopped), (7, Outcome::Stopped)]);
    }
}

/// `presentation.redemption_played` in the outbox: what one Twitch redemption showed and how it ended. Sent by the
/// module that showed it (check-in card, reward sound, reward media) once its item ends; simulated and test items
/// never send it. The redemption module keeps it with the record.
pub const REDEMPTION_PLAYED: &str = "presentation.redemption_played";

/// What a redemption showed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayedKind {
    Card,
    Sound,
    Media,
}

/// How it ended: an [`Outcome`], or `file_missing` for media whose file was gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayResult {
    Played,
    SoundMissing,
    FileMissing,
    OffAir,
    Declined,
    Skipped,
    Cleared,
    Stopped,
}

impl From<Outcome> for PlayResult {
    fn from(o: Outcome) -> Self {
        match o {
            Outcome::Played => Self::Played,
            Outcome::SoundMissing => Self::SoundMissing,
            Outcome::OffAir => Self::OffAir,
            Outcome::Declined => Self::Declined,
            Outcome::Skipped => Self::Skipped,
            Outcome::Cleared => Self::Cleared,
            Outcome::Stopped => Self::Stopped,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedemptionPlayed {
    /// The Twitch redemption ID.
    pub redemption_id: String,
    pub what: PlayedKind,
    pub result: PlayResult,
}

/// Sends a [`RedemptionPlayed`]; the host publishes it to the outbox.
pub type ReportPlayed = Arc<dyn Fn(RedemptionPlayed) + Send + Sync>;

/// Where a module sends [`RedemptionPlayed`]: nowhere until the host connects it, once the outbox exists (it is
/// built after the modules, since it takes their subscriptions).
#[derive(Clone, Default)]
pub struct PlayedReporter(Arc<std::sync::OnceLock<ReportPlayed>>);

impl PlayedReporter {
    /// The first connection stays.
    pub fn connect(&self, report: ReportPlayed) {
        let _ = self.0.set(report);
    }

    pub fn send(&self, played: RedemptionPlayed) {
        if let Some(report) = self.0.get() {
            report(played);
        }
    }
}

/// `done` that also reports what Twitch redemption `redemption` showed, with the result `result` makes of its
/// outcome.
pub fn reporting(
    done: Box<dyn FnOnce(Outcome) + Send>,
    report: PlayedReporter,
    redemption: String,
    what: PlayedKind,
    result: impl FnOnce(Outcome) -> PlayResult + Send + 'static,
) -> Box<dyn FnOnce(Outcome) + Send> {
    Box::new(move |outcome| {
        done(outcome);
        report.send(RedemptionPlayed {
            redemption_id: redemption,
            what,
            result: result(outcome),
        });
    })
}

#[cfg(test)]
mod played_tests {
    use super::*;

    #[test]
    fn reporting_records_the_outcome_then_reports_what_played_once_connected() {
        let reporter = PlayedReporter::default();
        let sent = Arc::new(Mutex::new(Vec::new()));
        let outcomes = Arc::new(Mutex::new(Vec::new()));
        let record = |outcomes: Arc<Mutex<Vec<Outcome>>>| -> Box<dyn FnOnce(Outcome) + Send> {
            Box::new(move |o| outcomes.lock().unwrap().push(o))
        };
        // Not connected yet: nothing is sent, the outcome is still recorded.
        reporting(
            record(outcomes.clone()),
            reporter.clone(),
            "r0".into(),
            PlayedKind::Sound,
            PlayResult::from,
        )(Outcome::Played);
        let into = sent.clone();
        reporter.connect(Arc::new(move |p| into.lock().unwrap().push(p)));
        reporting(
            record(outcomes.clone()),
            reporter,
            "r1".into(),
            PlayedKind::Media,
            |_| PlayResult::FileMissing,
        )(Outcome::Declined);
        assert_eq!(
            *outcomes.lock().unwrap(),
            vec![Outcome::Played, Outcome::Declined]
        );
        assert_eq!(
            *sent.lock().unwrap(),
            vec![RedemptionPlayed {
                redemption_id: "r1".into(),
                what: PlayedKind::Media,
                result: PlayResult::FileMissing,
            }]
        );
    }
}
