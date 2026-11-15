//! Receives alerts from Twitch or the simulator and publishes them to the overlay bus.

use std::sync::Arc;

use toolkit_core::{
    bus::{Event, EventBus},
    events::{Alert, Source},
    modules::{self, Modules},
};

use crate::infrastructure::{self, ALERT, ALERT_VERSION};

pub struct AlertsService {
    bus: Arc<EventBus>,
    modules: Arc<Modules>,
}

impl AlertsService {
    pub fn new(bus: Arc<EventBus>, modules: Arc<Modules>) -> Self {
        Self { bus, modules }
    }

    /// Dropped while the alerts module is disabled.
    pub fn receive(&self, alert: Alert, source: Source) {
        if !self.modules.is_enabled(modules::ALERTS) {
            return;
        }
        self.bus.publish(Event {
            id: alert.id.clone(),
            event_type: ALERT,
            version: ALERT_VERSION,
            occurred_at: alert.occurred_at.clone(),
            source,
            payload: infrastructure::payload(alert),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use toolkit_core::events::AlertKind;

    fn alert(id: &str, user_name: Option<&str>, kind: AlertKind) -> Alert {
        Alert {
            id: id.into(),
            user_name: user_name.map(Into::into),
            kind,
            occurred_at: "2026-11-16T10:00:00Z".into(),
        }
    }

    #[test]
    fn publishes_each_kind_only_while_enabled() {
        let dir = std::env::temp_dir().join(format!("st-alerts-{}", std::process::id()));
        let bus = Arc::new(EventBus::default());
        let modules = Arc::new(Modules::load(dir.clone()));
        let alerts = AlertsService::new(bus.clone(), modules.clone());
        let (_, mut rx) = bus.subscribe();

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
        modules.set_enabled(modules::ALERTS, false).unwrap();
        alerts.receive(alert("off", None, AlertKind::Follow), Source::Twitch);

        let mut sent = Vec::new();
        while let Ok(text) = rx.try_recv() {
            sent.push(serde_json::from_str::<Value>(&text).unwrap());
        }
        let payloads: Vec<&Value> = sent.iter().map(|e| &e["payload"]).collect();
        assert_eq!(sent.len(), 7, "the alert sent while disabled is dropped");
        assert_eq!(
            payloads[0],
            &serde_json::json!({"kind":"follow","userName":"Viewer"})
        );
        assert_eq!(
            payloads[2],
            &serde_json::json!({"kind":"resubscribe","userName":"Viewer","tier":1,"months":12,
                                "message":"<b>hi</b>"}),
            "viewer text stays raw"
        );
        assert_eq!(
            payloads[4],
            &serde_json::json!({"kind":"cheer","userName":"Viewer","bits":100}),
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
