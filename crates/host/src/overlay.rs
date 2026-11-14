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
        let wire: serde_json::Value = serde_json::from_str(&bus.subscribe().0).unwrap();
        // No missing, extra or renamed fields; a new retained event type must be added to SnapshotPayload.
        let typed: Snapshot = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap(), wire);
        let _ = std::fs::remove_dir_all(dir);
    }
}
