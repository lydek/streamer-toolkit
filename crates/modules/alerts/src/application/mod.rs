//! Receives alerts from Twitch or the simulator and plays them one at a time: each goes to the overlay bus, and
//! the next waits until it has shown.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::Notify;
use toolkit_core::{
    bus::{Event, EventBus},
    events::{Alert, AlertKind, Source},
    modules::{self, Modules},
};

use crate::infrastructure::{self, ALERT, ALERT_VERSION};

/// shortcut: beyond this many waiting alerts the oldest is dropped (a follow bot could otherwise queue hours of
/// alerts); raise it if real streams ever reach it.
const WAITING_MAX: usize = 100;

/// How long an alert shows and the pause before the next one.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub show: Duration,
    /// For a resub or cheer that carries a message to read.
    pub show_with_message: Duration,
    pub gap: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            show: Duration::from_secs(6),
            show_with_message: Duration::from_secs(9),
            gap: Duration::from_secs(1),
        }
    }
}

pub struct AlertsService(Arc<Queue>);

struct Queue {
    bus: Arc<EventBus>,
    modules: Arc<Modules>,
    timing: Timing,
    waiting: Mutex<VecDeque<(Alert, Source)>>,
    ready: Notify,
}

impl AlertsService {
    /// Starts the player task; needs a Tokio runtime.
    pub fn new(bus: Arc<EventBus>, modules: Arc<Modules>, timing: Timing) -> Self {
        let queue = Arc::new(Queue {
            bus,
            modules,
            timing,
            waiting: Mutex::default(),
            ready: Notify::new(),
        });
        tokio::spawn(play(queue.clone()));
        Self(queue)
    }

    /// Queues `alert`. A gifted sub is covered by its gift's alert, so one gift of N plays once.
    pub fn receive(&self, alert: Alert, source: Source) {
        if matches!(alert.kind, AlertKind::Subscribe { gifted: true, .. }) {
            return;
        }
        let mut waiting = self.0.waiting.lock().unwrap();
        if waiting.len() == WAITING_MAX {
            waiting.pop_front();
            tracing::warn!(
                code = "alerts.queue_full",
                "dropped the oldest waiting alert"
            );
        }
        waiting.push_back((alert, source));
        drop(waiting);
        self.0.ready.notify_one();
    }
}

/// Publishes the waiting alerts in order, each after the previous one has shown. Alerts reaching the front
/// while the module is disabled are dropped.
async fn play(queue: Arc<Queue>) {
    loop {
        let next = queue.waiting.lock().unwrap().pop_front();
        let Some((alert, source)) = next else {
            // A notify sent since the queue was found empty is kept, so nothing waits forever.
            queue.ready.notified().await;
            continue;
        };
        if !queue.modules.is_enabled(modules::ALERTS) {
            continue;
        }
        let has_message = matches!(
            &alert.kind,
            AlertKind::Resubscribe { message, .. } | AlertKind::Cheer { message, .. } if !message.is_empty()
        );
        let show = match has_message {
            true => queue.timing.show_with_message,
            false => queue.timing.show,
        };
        queue.bus.publish(Event {
            id: alert.id.clone(),
            event_type: ALERT,
            version: ALERT_VERSION,
            occurred_at: alert.occurred_at.clone(),
            source,
            payload: infrastructure::payload(alert, show),
        });
        tokio::time::sleep(show + queue.timing.gap).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tokio::{sync::broadcast, time::Instant};

    fn alert(id: &str, user_name: Option<&str>, kind: AlertKind) -> Alert {
        Alert {
            id: id.into(),
            user_name: user_name.map(Into::into),
            kind,
            occurred_at: "2026-11-16T10:00:00Z".into(),
        }
    }

    fn setup(
        name: &str,
    ) -> (
        std::path::PathBuf,
        Arc<Modules>,
        AlertsService,
        broadcast::Receiver<Arc<str>>,
    ) {
        let dir = std::env::temp_dir().join(format!("st-alerts-{name}-{}", std::process::id()));
        let bus = Arc::new(EventBus::default());
        let modules = Arc::new(Modules::load(dir.clone()));
        let alerts = AlertsService::new(bus.clone(), modules.clone(), Timing::default());
        let (_, rx) = bus.subscribe();
        (dir, modules, alerts, rx)
    }

    async fn next(rx: &mut broadcast::Receiver<Arc<str>>) -> Value {
        serde_json::from_str(&rx.recv().await.unwrap()).unwrap()
    }

    // Paused time: the clock jumps ahead whenever every task waits, so the pacing is checked exactly.
    #[tokio::test(start_paused = true)]
    async fn plays_one_at_a_time_in_order_and_a_gift_once() {
        let (dir, _, alerts, mut rx) = setup("queue");
        let start = Instant::now();
        alerts.receive(
            alert("follow", Some("Fan"), AlertKind::Follow),
            Source::Twitch,
        );
        alerts.receive(
            alert("gift", None, AlertKind::Gift { tier: 1, count: 3 }),
            Source::Twitch,
        );
        for n in 0..3 {
            let gifted = AlertKind::Subscribe {
                tier: 1,
                gifted: true,
            };
            alerts.receive(
                alert(&format!("gifted-{n}"), Some("Lucky"), gifted),
                Source::Twitch,
            );
        }
        alerts.receive(
            alert(
                "cheer",
                Some("Cheerer"),
                AlertKind::Cheer {
                    bits: 100,
                    message: "hi".into(),
                },
            ),
            Source::Twitch,
        );
        alerts.receive(
            alert("raid", Some("Raider"), AlertKind::Raid { viewers: 5 }),
            Source::Simulation,
        );
        alerts.receive(
            alert(
                "sub",
                Some("Sub"),
                AlertKind::Subscribe {
                    tier: 1,
                    gifted: false,
                },
            ),
            Source::Twitch,
        );

        let mut played = Vec::new();
        for _ in 0..5 {
            let event = next(&mut rx).await;
            played.push((
                event["id"].as_str().unwrap().to_owned(),
                start.elapsed().as_secs(),
                event,
            ));
        }
        let order: Vec<(&str, u64)> = played
            .iter()
            .map(|(id, at, _)| (id.as_str(), *at))
            .collect();
        // 6 s + 1 s each; the cheer has a message, so 9 s + 1 s. The gifted subs never play.
        assert_eq!(
            order,
            [
                ("follow", 0),
                ("gift", 7),
                ("cheer", 14),
                ("raid", 24),
                ("sub", 31)
            ]
        );
        assert_eq!(played[1].2["payload"]["durationMs"], 6000);
        assert_eq!(played[2].2["payload"]["durationMs"], 9000);
        assert_eq!(played[3].2["source"], "simulation");
        assert!(
            tokio::time::timeout(Duration::from_secs(60), rx.recv())
                .await
                .is_err(),
            "nothing else plays"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(start_paused = true)]
    async fn publishes_each_kind_only_while_enabled() {
        let (dir, modules, alerts, mut rx) = setup("kinds");
        let kinds = [
            AlertKind::Follow,
            AlertKind::Subscribe {
                tier: 2,
                gifted: false,
            },
            AlertKind::Resubscribe {
                tier: 1,
                months: 12,
                message: "<b>hi</b>".into(),
            },
            AlertKind::Gift { tier: 1, count: 20 },
            AlertKind::Cheer {
                bits: 100,
                message: String::new(),
            },
            AlertKind::Raid { viewers: 42 },
        ];
        for (i, kind) in kinds.into_iter().enumerate() {
            alerts.receive(alert(&i.to_string(), Some("Viewer"), kind), Source::Twitch);
        }
        alerts.receive(
            alert("anon", None, AlertKind::Gift { tier: 3, count: 1 }),
            Source::Simulation,
        );
        let mut sent = Vec::new();
        for _ in 0..7 {
            sent.push(next(&mut rx).await);
        }
        modules.set_enabled(modules::ALERTS, false).unwrap();
        alerts.receive(alert("off", None, AlertKind::Follow), Source::Twitch);
        assert!(
            tokio::time::timeout(Duration::from_secs(60), rx.recv())
                .await
                .is_err(),
            "the alert sent while disabled is dropped"
        );

        let payloads: Vec<&Value> = sent.iter().map(|e| &e["payload"]).collect();
        assert_eq!(
            payloads[0],
            &serde_json::json!({"kind":"follow","userName":"Viewer","durationMs":6000})
        );
        assert_eq!(
            payloads[2],
            &serde_json::json!({"kind":"resubscribe","userName":"Viewer","tier":1,"months":12,
                                "message":"<b>hi</b>","durationMs":9000}),
            "viewer text stays raw"
        );
        assert_eq!(
            payloads[4],
            &serde_json::json!({"kind":"cheer","userName":"Viewer","bits":100,"durationMs":6000}),
            "an empty message is left out"
        );
        assert_eq!(payloads[6]["userName"], Value::Null);
        assert_eq!(sent[6]["source"], "simulation");
        assert!(sent.iter().all(|e| e["type"] == "alert"));
        // The OpenAPI type must describe the wire format exactly: no missing, extra or renamed fields.
        for event in &sent {
            let typed: crate::AlertEvent = serde_json::from_value(event.clone()).unwrap();
            assert_eq!(&serde_json::to_value(typed).unwrap(), event);
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
