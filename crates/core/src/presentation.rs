//! One queue for what the overlays present (alerts, stamp cards, reward sounds): items play one at a time, in the order they
//! came, with no size limit, so nothing overlaps and nothing is dropped. Modules still decide what to show; the
//! queue only paces it.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::Notify;

/// The pause between two items.
pub const DEFAULT_GAP: Duration = Duration::from_secs(1);

/// The overlay that presents an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Alerts,
    Checkin,
    Sound,
}

pub struct Item {
    pub stage: Stage,
    /// How long the overlay shows it.
    pub show: Duration,
    /// From the simulator: shown even when the overlay is not open in OBS, so the admin window's preview still
    /// gets it.
    pub simulated: bool,
    /// Publishes it to the overlay; `false` skips it after all (e.g. its module was turned off while it waited).
    pub present: Box<dyn FnOnce() -> bool + Send>,
}

type OnAir = Box<dyn Fn(Stage) -> bool + Send + Sync>;

pub struct Presenter {
    waiting: Mutex<VecDeque<Item>>,
    ready: Notify,
    gap: Duration,
    on_air: OnAir,
}

impl Presenter {
    /// Starts the player; `on_air` says whether a stage's overlay is open in OBS right now. Needs a Tokio runtime.
    pub fn start(
        gap: Duration,
        on_air: impl Fn(Stage) -> bool + Send + Sync + 'static,
    ) -> Arc<Self> {
        let presenter = Arc::new(Self {
            waiting: Mutex::default(),
            ready: Notify::new(),
            gap,
            on_air: Box::new(on_air),
        });
        tokio::spawn(play(presenter.clone()));
        presenter
    }

    /// Queues `item` behind everything already waiting.
    pub fn push(&self, item: Item) {
        self.waiting.lock().unwrap().push_back(item);
        self.ready.notify_one();
    }
}

/// Presents the waiting items in order, each after the previous one has shown. An item whose overlay is not open
/// is skipped without waiting: nobody would see it.
async fn play(presenter: Arc<Presenter>) {
    loop {
        let next = presenter.waiting.lock().unwrap().pop_front();
        let Some(item) = next else {
            // A notify sent since the queue was found empty is kept, so nothing waits forever.
            presenter.ready.notified().await;
            continue;
        };
        if !item.simulated && !(presenter.on_air)(item.stage) {
            tracing::debug!(stage = ?item.stage, "overlay not open, item skipped");
            continue;
        }
        if (item.present)() {
            tokio::time::sleep(item.show + presenter.gap).await;
        }
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
            simulated: false,
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
            simulated: false,
            present: Box::new(|| false),
        });
        presenter.push(item(Stage::Alerts, 1000, &tx, 1));
        assert_eq!(rx.recv().await.unwrap(), (1, start));
    }
}
