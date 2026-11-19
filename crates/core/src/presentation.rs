//! One queue for what the overlays present (alerts, stamp cards, reward sounds): items play one at a time, in the
//! order they came, with no size limit, so nothing overlaps and nothing is dropped. Modules still decide what to
//! show; the queue only paces it. The broadcaster can clear what waits, or stop everything at once.

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

pub struct Item {
    pub stage: Stage,
    /// What the queue area shows for it, e.g. the viewer's name or the sound's label; raw text.
    pub label: String,
    /// How long the overlay shows it.
    pub show: Duration,
    /// From the simulator: shown even when the overlay is not open in OBS, so the admin window's preview still
    /// gets it.
    pub simulated: bool,
    /// Work it needs before it can show (a stamp card's avatar lookup); it holds its place meanwhile, and a stop
    /// ends it.
    pub ready: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
    /// Publishes it to the overlay; `false` skips it after all (e.g. its module was turned off while it waited).
    pub present: Box<dyn FnOnce() -> bool + Send>,
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
    /// Raw text.
    pub label: String,
    pub show_ms: u64,
    /// RFC 3339 time it started.
    pub started_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Waiting {
    pub stage: Stage,
    /// Raw text.
    pub label: String,
    pub show_ms: u64,
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
        let cancelled = state.waiting.len();
        state.waiting.clear();
        self.publish(&state);
        cancelled
    }

    /// Drops every waiting item and ends the one playing now; the caller tells the overlays to stop at once.
    /// Returns whether one was playing and how many waiting ones were dropped.
    ///
    /// `announce` tells the overlays (gets the same two values) while the queue is still locked, so nothing taken
    /// after the stop can reach an overlay before the stop does.
    pub fn stop(&self, announce: impl FnOnce(bool, usize)) -> (bool, usize) {
        let mut state = self.state.lock().unwrap();
        let cancelled = state.waiting.len();
        state.waiting.clear();
        let playing = state.current.take().is_some();
        self.publish(&state);
        self.stops.send_modify(|n| *n += 1);
        announce(playing, cancelled);
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
                    label: i.label.clone(),
                    show_ms: millis(i.show),
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
/// is skipped without waiting: nobody would see it. An emergency stop ends the wait of the one playing.
async fn play(presenter: Arc<Presenter>) {
    loop {
        // Taken, checked and marked as playing in one step under the lock stop() takes, with the stop signal read
        // there too: a stop after this ends the item, a stop before it never sees it.
        let next = {
            let mut state = presenter.state.lock().unwrap();
            match state.waiting.pop_front() {
                None => None,
                Some(mut item) => {
                    let taken = if item.simulated || (presenter.on_air)(item.stage) {
                        state.current = Some(Playing {
                            stage: item.stage,
                            label: std::mem::take(&mut item.label),
                            show_ms: millis(item.show),
                            started_at: chrono::Utc::now()
                                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        });
                        Some((item, presenter.stops.subscribe()))
                    } else {
                        tracing::debug!(stage = ?item.stage, "overlay not open, item skipped");
                        None
                    };
                    presenter.publish(&state);
                    Some(taken)
                }
            }
        };
        let (item, mut stops) = match next {
            Some(Some(taken)) => taken,
            Some(None) => continue,
            None => {
                // A notify sent since the queue was found empty is kept, so nothing waits forever.
                presenter.ready.notified().await;
                continue;
            }
        };
        let mut stopped = false;
        if let Some(ready) = item.ready {
            tokio::select! {
                _ = ready => {}
                _ = stops.changed() => stopped = true,
            }
        }
        // Checked and published under the lock stop() takes, so a stop is never followed by this item's picture.
        let shown = {
            let _state = presenter.state.lock().unwrap();
            !stopped && !stops.has_changed().unwrap_or(false) && (item.present)()
        };
        if shown {
            tokio::select! {
                _ = tokio::time::sleep(item.show + presenter.gap) => {}
                _ = stops.changed() => {}
            }
        }
        let mut state = presenter.state.lock().unwrap();
        state.current = None;
        presenter.publish(&state);
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
            label: "item".into(),
            simulated: false,
            ready: None,
            present: Box::new(move || tx.send((n, Instant::now())).is_ok()),
        }
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
            label: "item".into(),
            simulated: false,
            ready: None,
            present: Box::new(|| false),
        });
        presenter.push(item(Stage::Alerts, 1000, &tx, 1));
        assert_eq!(rx.recv().await.unwrap(), (1, start));
    }

    #[tokio::test(start_paused = true)]
    async fn the_status_follows_what_plays_and_what_waits() {
        let presenter = Presenter::start(Duration::from_millis(100), |_| true);
        let mut status = presenter.subscribe();
        let (tx, mut rx) = mpsc::unbounded_channel();
        presenter.push(Item {
            label: "Fan".into(),
            ..item(Stage::Alerts, 1000, &tx, 0)
        });
        presenter.push(Item {
            label: "Bell".into(),
            ..item(Stage::Sound, 2000, &tx, 1)
        });
        rx.recv().await.unwrap();
        let now = status.borrow_and_update().clone();
        let current = now.current.unwrap();
        assert_eq!(
            (current.stage, current.label.as_str(), current.show_ms),
            (Stage::Alerts, "Fan", 1000)
        );
        assert_eq!(
            now.waiting,
            [Waiting {
                stage: Stage::Sound,
                label: "Bell".into(),
                show_ms: 2000
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
                tx2.send((1, Instant::now())).is_ok()
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
}
