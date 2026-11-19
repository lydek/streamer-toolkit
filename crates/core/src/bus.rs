//! In-memory event bus for screen events (chat, simulations, connection state). Nothing here is persisted.
//! Every event gets the external envelope and a `seq`; clients take a snapshot first, then follow `seq`.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::broadcast;

use crate::events::Source;

const CHANNEL_CAPACITY: usize = 256;

/// External event envelope (docs/architecture.md §5).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub version: u32,
    pub seq: u64,
    pub occurred_at: String,
    pub source: Source,
    pub payload: Value,
}

/// What a module hands to the bus; the bus assigns `seq`.
pub struct Event {
    pub id: String,
    pub event_type: &'static str,
    pub version: u32,
    pub occurred_at: String,
    pub source: Source,
    pub payload: Value,
}

pub struct EventBus {
    tx: broadcast::Sender<Arc<str>>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    seq: u64,
    /// Recent events per type that go into snapshots, with how many to keep.
    retained: HashMap<&'static str, (usize, VecDeque<Envelope>)>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self {
            tx: broadcast::channel(CHANNEL_CAPACITY).0,
            state: Mutex::default(),
        }
    }
}

impl EventBus {
    /// Keeps the last `keep` events of `event_type` for snapshots.
    pub fn retain(&self, event_type: &'static str, keep: usize) {
        self.state
            .lock()
            .unwrap()
            .retained
            .insert(event_type, (keep, VecDeque::new()));
    }

    pub fn publish(&self, event: Event) -> u64 {
        // Sent under the lock so `seq` order is send order and a snapshot never misses or repeats an event.
        let mut state = self.state.lock().unwrap();
        state.seq += 1;
        let envelope = Envelope {
            id: event.id,
            event_type: event.event_type,
            version: event.version,
            seq: state.seq,
            occurred_at: event.occurred_at,
            source: event.source,
            payload: event.payload,
        };
        let text: Arc<str> = serde_json::to_string(&envelope)
            .expect("envelope serializes")
            .into();
        let _ = self.tx.send(text);
        if let Some((keep, recent)) = state.retained.get_mut(envelope.event_type) {
            if recent.len() == *keep {
                recent.pop_front();
            }
            recent.push_back(envelope);
        }
        state.seq
    }

    /// Publishes `event` and, in the same step, drops the retained `retained_type` events `gone` matches, so no
    /// snapshot taken after it still has them (e.g. chat messages a moderator deleted).
    pub fn publish_dropping(
        &self,
        event: Event,
        retained_type: &'static str,
        gone: impl Fn(&Envelope) -> bool,
    ) -> u64 {
        let mut state = self.state.lock().unwrap();
        if let Some((_, recent)) = state.retained.get_mut(retained_type) {
            recent.retain(|e| !gone(e));
        }
        drop(state);
        // Not atomic with the drop above, but a snapshot in between only lacks messages that are gone anyway.
        self.publish(event)
    }

    /// Subscribes and takes the snapshot atomically: `{"type":"snapshot","seq":N,"payload":{<type>:[...]}}`.
    pub fn subscribe(&self) -> (String, broadcast::Receiver<Arc<str>>) {
        let state = self.state.lock().unwrap();
        let retained: HashMap<_, _> = state
            .retained
            .iter()
            .map(|(event_type, (_, recent))| (*event_type, recent))
            .collect();
        let snapshot = json!({
            "type": "snapshot",
            "version": 1,
            "seq": state.seq,
            "payload": retained,
        });
        (snapshot.to_string(), self.tx.subscribe())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str, event_type: &'static str) -> Event {
        Event {
            id: id.into(),
            event_type,
            version: 1,
            occurred_at: "2026-11-13T10:00:00Z".into(),
            source: Source::Simulation,
            payload: json!({ "text": "<b>hi</b>" }),
        }
    }

    #[test]
    fn snapshot_keeps_retained_events_and_events_follow_seq() {
        let bus = EventBus::default();
        bus.retain("chat.message", 2);
        bus.publish(event("a", "chat.message"));
        bus.publish(event("x", "other"));
        bus.publish(event("b", "chat.message"));
        bus.publish(event("c", "chat.message"));

        let (snapshot, mut rx) = bus.subscribe();
        let snapshot: Value = serde_json::from_str(&snapshot).unwrap();
        assert_eq!(snapshot["seq"], 4);
        let chat = snapshot["payload"]["chat.message"].as_array().unwrap();
        assert_eq!(chat.len(), 2);
        assert_eq!(chat[0]["id"], "b");
        assert!(
            snapshot["payload"].get("other").is_none(),
            "only retained types"
        );

        bus.publish(event("d", "chat.message"));
        let next: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(next["seq"], 5);
        assert_eq!(next["type"], "chat.message");
        assert_eq!(next["source"], "simulation");
        assert_eq!(next["occurredAt"], "2026-11-13T10:00:00Z");
        // Viewer text stays raw data; overlays escape it when rendering.
        assert_eq!(next["payload"]["text"], "<b>hi</b>");
    }

    #[test]
    fn publish_dropping_takes_matching_events_out_of_later_snapshots() {
        let bus = EventBus::default();
        bus.retain("chat.message", 5);
        bus.publish(event("a", "chat.message"));
        bus.publish(event("b", "chat.message"));
        bus.publish_dropping(event("r", "chat.removed"), "chat.message", |e| e.id == "a");
        let (snapshot, _) = bus.subscribe();
        let snapshot: Value = serde_json::from_str(&snapshot).unwrap();
        let ids: Vec<&str> = snapshot["payload"]["chat.message"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["b"]);
        assert_eq!(snapshot["seq"], 3, "the removal itself was published");
    }
}
