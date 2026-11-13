//! Read-only overlay channel: the first WebSocket message is a snapshot, then events follow in `seq` order.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::Response,
};
use integrations::twitch::eventsub::{ChatMessage, Fragment};
use serde_json::{Value, json};
use tokio::sync::broadcast;

/// Recent chat kept in memory only, so a reloaded OBS source is not empty.
const RECENT_CHAT: usize = 50;
const CHANNEL_CAPACITY: usize = 256;

pub(crate) struct OverlayHub {
    tx: broadcast::Sender<Arc<str>>,
    state: Mutex<HubState>,
}

#[derive(Default)]
struct HubState {
    seq: u64,
    recent_chat: VecDeque<Value>,
}

impl OverlayHub {
    pub(crate) fn new() -> Self {
        Self {
            tx: broadcast::channel(CHANNEL_CAPACITY).0,
            state: Mutex::default(),
        }
    }

    pub(crate) fn publish_chat(&self, message: ChatMessage) {
        // Sent under the lock so `seq` order is send order and a snapshot never misses or repeats an event.
        let mut state = self.state.lock().unwrap();
        state.seq += 1;
        let event = chat_event(state.seq, message);
        if state.recent_chat.len() == RECENT_CHAT {
            state.recent_chat.pop_front();
        }
        let _ = self.tx.send(event.to_string().into());
        state.recent_chat.push_back(event);
    }

    /// Subscribes and takes the snapshot atomically.
    fn subscribe(&self) -> (String, broadcast::Receiver<Arc<str>>) {
        let state = self.state.lock().unwrap();
        let snapshot = json!({
            "type": "snapshot",
            "version": 1,
            "seq": state.seq,
            "payload": { "chat": state.recent_chat },
        });
        (snapshot.to_string(), self.tx.subscribe())
    }
}

/// External event envelope (docs/architecture.md §5); the internal type never leaves the Host as-is.
fn chat_event(seq: u64, m: ChatMessage) -> Value {
    let fragments: Vec<Value> = m
        .fragments
        .into_iter()
        .map(|f| match f {
            Fragment::Text(text) => json!({ "type": "text", "text": text }),
            Fragment::Emote { id, text } => json!({ "type": "emote", "text": text, "emoteId": id }),
        })
        .collect();
    json!({
        "id": m.id,
        "type": "chat.message",
        "version": 1,
        "seq": seq,
        "occurredAt": m.sent_at,
        "source": "twitch",
        "payload": {
            "userId": m.user_id,
            "userLogin": m.user_login,
            "userName": m.user_name,
            "color": m.color,
            "text": m.text,
            "fragments": fragments,
        },
    })
}

pub(crate) async fn ws(ws: WebSocketUpgrade, State(hub): State<Arc<OverlayHub>>) -> Response {
    ws.on_upgrade(move |socket| serve(socket, hub))
}

async fn serve(mut socket: WebSocket, hub: Arc<OverlayHub>) {
    let (snapshot, mut events) = hub.subscribe();
    if socket.send(Message::Text(snapshot.into())).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(text) => {
                    if socket.send(Message::Text(text.as_ref().into())).await.is_err() {
                        return;
                    }
                }
                // Lagged: close so the overlay reconnects and resyncs from a fresh snapshot.
                Err(_) => return,
            },
            // Incoming messages are ignored: the channel is read-only.
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                Some(Ok(_)) => {}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: &str) -> ChatMessage {
        ChatMessage {
            id: id.into(),
            user_id: "7".into(),
            user_login: "viewer".into(),
            user_name: "Viewer".into(),
            color: None,
            text: "<b>hi</b>".into(),
            fragments: vec![Fragment::Text("<b>hi</b>".into())],
            sent_at: "2026-11-13T10:00:00Z".into(),
        }
    }

    #[test]
    fn snapshot_keeps_recent_chat_and_events_follow_seq() {
        let hub = OverlayHub::new();
        for i in 0..RECENT_CHAT + 2 {
            hub.publish_chat(message(&format!("c{i}")));
        }
        let (snapshot, mut rx) = hub.subscribe();
        let snapshot: Value = serde_json::from_str(&snapshot).unwrap();
        assert_eq!(snapshot["seq"], (RECENT_CHAT + 2) as u64);
        let chat = snapshot["payload"]["chat"].as_array().unwrap();
        assert_eq!(chat.len(), RECENT_CHAT);
        assert_eq!(chat[0]["id"], "c2");

        hub.publish_chat(message("next"));
        let event: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(event["seq"], (RECENT_CHAT + 3) as u64);
        assert_eq!(event["type"], "chat.message");
        assert_eq!(event["source"], "twitch");
        assert_eq!(event["occurredAt"], "2026-11-13T10:00:00Z");
        // Viewer text stays raw data; the overlay escapes it when rendering.
        assert_eq!(event["payload"]["text"], "<b>hi</b>");
    }
}
