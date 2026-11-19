//! Receives alerts from Twitch or the simulator and queues them on the shared presentation queue, which hands
//! each on to the overlays once the one before has shown.

use std::{sync::Arc, time::Duration};

use toolkit_core::{
    events::{Alert, AlertKind, Source},
    modules::{self, Modules},
    presentation::{Item, Presenter, Stage},
};

/// How long an alert shows; the pause before the next item belongs to the presentation queue.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub show: Duration,
    /// For a resub or cheer that carries a message to read.
    pub show_with_message: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            show: Duration::from_secs(6),
            show_with_message: Duration::from_secs(9),
        }
    }
}

/// Shows an alert on the overlays for the given time; `infrastructure` publishes it on the bus.
/// Tells the admin window an alert arrived; `infrastructure` publishes it on the bus.
type Announce = Box<dyn Fn(&Alert, Source, Duration) + Send + Sync>;

type Publish = Arc<dyn Fn(Alert, Source, Duration) + Send + Sync>;

pub struct AlertsService {
    modules: Arc<Modules>,
    timing: Timing,
    presenter: Arc<Presenter>,
    announce: Announce,
    publish: Publish,
}

impl AlertsService {
    /// `announce` tells the admin window an alert arrived, at once and whether or not it will play; `publish` hands
    /// it to the overlays when its turn comes.
    pub(crate) fn with_publisher(
        modules: Arc<Modules>,
        timing: Timing,
        presenter: Arc<Presenter>,
        announce: impl Fn(&Alert, Source, Duration) + Send + Sync + 'static,
        publish: impl Fn(Alert, Source, Duration) + Send + Sync + 'static,
    ) -> Self {
        Self {
            modules,
            timing,
            presenter,
            announce: Box::new(announce),
            publish: Arc::new(publish),
        }
    }

    /// Announces `alert` and queues it. A gifted sub is covered by its gift's alert, so one gift of N plays once.
    /// An alert reaching the front while the module is disabled is dropped.
    pub fn receive(&self, alert: Alert, source: Source) {
        if matches!(alert.kind, AlertKind::Subscribe { gifted: true, .. }) {
            return;
        }
        let has_message = matches!(
            &alert.kind,
            AlertKind::Resubscribe { message, .. } | AlertKind::Cheer { message, .. } if !message.is_empty()
        );
        let show = match has_message {
            true => self.timing.show_with_message,
            false => self.timing.show,
        };
        (self.announce)(&alert, source, show);
        let (modules, publish) = (self.modules.clone(), self.publish.clone());
        self.presenter.push(Item {
            stage: Stage::Alerts,
            label: alert.user_name.clone().unwrap_or_default(),
            show,
            simulated: source == Source::Simulation,
            ready: None,
            present: Box::new(move || {
                if !modules.is_enabled(modules::ALERTS) {
                    return false;
                }
                publish(alert, source, show);
                true
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use sound::SoundService;
    use tokio::{sync::broadcast, time::Instant};
    use toolkit_core::bus::EventBus;

    fn alert(id: &str, user_name: Option<&str>, kind: AlertKind) -> Alert {
        Alert {
            id: id.into(),
            user_id: user_name.map(|_| "7".into()),
            user_login: user_name.map(str::to_lowercase),
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
        let presenter = Presenter::start(toolkit_core::presentation::DEFAULT_GAP, |_| true);
        let sounds = Arc::new(SoundService::new(
            dir.clone(),
            modules.clone(),
            bus.clone(),
            presenter.clone(),
        ));
        let alerts = AlertsService::new(
            bus.clone(),
            modules.clone(),
            Timing::default(),
            presenter,
            sounds,
        );
        let (_, rx) = bus.subscribe();
        (dir, modules, alerts, rx)
    }

    const SOUND_ID: &str = "0190a5b0-0000-7000-8000-000000000001";

    /// The next alert handed to the overlays (announcements skipped).
    async fn next(rx: &mut broadcast::Receiver<Arc<str>>) -> Value {
        loop {
            let event: Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
            if event["type"] == "alert" {
                return event;
            }
        }
    }

    #[tokio::test]
    async fn an_alert_is_announced_at_once_even_when_no_overlay_plays_it() {
        let dir = std::env::temp_dir().join(format!("st-alerts-announce-{}", std::process::id()));
        let bus = Arc::new(EventBus::default());
        let modules = Arc::new(Modules::load(dir.clone()));
        // No alerts overlay in OBS: nothing plays.
        let presenter = Presenter::start(toolkit_core::presentation::DEFAULT_GAP, |_| false);
        let sounds = Arc::new(SoundService::new(
            dir.clone(),
            modules.clone(),
            bus.clone(),
            presenter.clone(),
        ));
        let alerts = AlertsService::new(bus.clone(), modules, Timing::default(), presenter, sounds);
        let (_, mut rx) = bus.subscribe();
        alerts.receive(
            alert("follow", Some("Fan"), AlertKind::Follow),
            Source::Twitch,
        );
        let event: Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
        assert_eq!(
            (
                event["type"].as_str(),
                event["payload"]["userName"].as_str()
            ),
            (Some("alert.received"), Some("Fan"))
        );
        let typed: crate::AlertReceivedEvent = serde_json::from_value(event.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(typed).unwrap(),
            event,
            "the schema type matches the bus"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(200), rx.recv())
                .await
                .is_err(),
            "and it never plays"
        );
        let _ = std::fs::remove_dir_all(dir);
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
        // Follows play a library sound, cheers none; other kinds keep the built-in one.
        toolkit_core::settings::update(&dir, |s| {
            s.sound.library.push(toolkit_core::settings::SoundEntry {
                id: SOUND_ID.into(),
                label: "Ding".into(),
                volume: 40,
                format: "wav".into(),
                duration_ms: None,
            });
            s.sound.slots.insert("alert.follow".into(), SOUND_ID.into());
            s.sound.slots.insert("alert.cheer".into(), "silent".into());
        })
        .unwrap();
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
        // Still announced (it happened), but never played.
        assert!(
            tokio::time::timeout(Duration::from_secs(60), next(&mut rx))
                .await
                .is_err(),
            "the alert sent while disabled is dropped"
        );

        let payloads: Vec<&Value> = sent.iter().map(|e| &e["payload"]).collect();
        assert_eq!(
            payloads[0],
            &serde_json::json!({"kind":"follow","userId":"7","userLogin":"viewer","userName":"Viewer","durationMs":6000,
                                "sound":{"kind":"file","url":format!("/sounds/{SOUND_ID}"),"volume":40}})
        );
        assert_eq!(
            payloads[2],
            &serde_json::json!({"kind":"resubscribe","userId":"7","userLogin":"viewer","userName":"Viewer","tier":1,"months":12,
                                "message":"<b>hi</b>","durationMs":9000,"sound":{"kind":"built_in"}}),
            "viewer text stays raw"
        );
        assert_eq!(
            payloads[4],
            &serde_json::json!({"kind":"cheer","userId":"7","userLogin":"viewer","userName":"Viewer","bits":100,"durationMs":6000,
                                "sound":{"kind":"silent"}}),
            "an empty message is left out"
        );
        assert_eq!(payloads[6]["userName"], Value::Null);
        assert_eq!(
            (&payloads[6]["userId"], &payloads[6]["userLogin"]),
            (&Value::Null, &Value::Null)
        );
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
