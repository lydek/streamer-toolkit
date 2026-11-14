//! Read-only overlay channel: the first WebSocket message is a snapshot, then events follow in `seq` order.

use std::sync::Arc;

use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::Response,
};
use chat::ChatMessageEvent;

use crate::obs::ObsStatusEvent;
use serde::{Deserialize, Serialize};
use toolkit_core::bus::EventBus;
use utoipa::ToSchema;

pub(crate) async fn ws(ws: WebSocketUpgrade, State(bus): State<Arc<EventBus>>) -> Response {
    ws.on_upgrade(move |socket| serve(socket, bus))
}

async fn serve(mut socket: WebSocket, bus: Arc<EventBus>) {
    let (snapshot, mut events) = bus.subscribe();
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

/// The first message on `/ws/overlay`: the recent retained events of each type.
// The bus builds the snapshot itself; this type documents it in OpenAPI and a test keeps the two in step.
#[derive(Serialize, Deserialize, ToSchema)]
pub(crate) struct Snapshot {
    #[serde(rename = "type")]
    kind: SnapshotType,
    version: u32,
    seq: u64,
    payload: SnapshotPayload,
}

#[derive(Serialize, Deserialize, ToSchema)]
enum SnapshotType {
    #[serde(rename = "snapshot")]
    Snapshot,
}

#[derive(Serialize, Deserialize, ToSchema)]
struct SnapshotPayload {
    #[serde(rename = "chat.message")]
    chat_message: Vec<ChatMessageEvent>,
    /// At most one: the latest status.
    #[serde(rename = "obs.status")]
    obs_status: Vec<ObsStatusEvent>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat::ChatService;
    use toolkit_core::{
        events::{ChatMessage, Fragment, Source},
        modules::Modules,
    };

    #[test]
    fn snapshot_type_matches_the_bus() {
        let dir = std::env::temp_dir().join(format!("st-overlay-snapshot-{}", std::process::id()));
        let bus = Arc::new(EventBus::default());
        let chat = ChatService::new(bus.clone(), Arc::new(Modules::load(dir.clone())));
        chat.receive(
            ChatMessage {
                id: "a".into(),
                user_id: "7".into(),
                user_login: "viewer".into(),
                user_name: "Viewer".into(),
                color: None,
                text: "hi".into(),
                fragments: vec![Fragment::Text("hi".into())],
                sent_at: "2026-11-13T10:00:00Z".into(),
            },
            Source::Simulation,
        );
        crate::obs::retain(&bus);
        crate::obs::publish(&bus, integrations::obs::ObsStatus::CONNECTING);
        let wire: serde_json::Value = serde_json::from_str(&bus.subscribe().0).unwrap();
        // No missing, extra or renamed fields; a new retained event type must be added to SnapshotPayload.
        let typed: Snapshot = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap(), wire);

        // The generated schema must agree too: every field it marks required is on the wire.
        let event = &wire["payload"]["chat.message"][0];
        assert_required("Snapshot", &wire);
        assert_required("SnapshotPayload", &wire["payload"]);
        assert_required("ChatMessageEvent", event);
        assert_required("ChatMessagePayload", &event["payload"]);
        let obs = &wire["payload"]["obs.status"][0];
        assert_required("ObsStatusEvent", obs);
        assert_required("ObsStatusPayload", &obs["payload"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn assert_required(schema: &str, value: &serde_json::Value) {
        let doc = serde_json::to_value(crate::openapi()).unwrap();
        let required = doc["components"]["schemas"][schema]["required"]
            .as_array()
            .unwrap_or_else(|| panic!("{schema} has a required list"));
        for key in required {
            let key = key.as_str().unwrap();
            assert!(
                value.get(key).is_some(),
                "{schema}.{key} is required but not sent"
            );
        }
    }
}
