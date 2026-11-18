//! Read-only overlay channel: the first WebSocket message is a snapshot, then events follow in `seq` order.
//! Overlay pages connect with `?kind=chat|alerts|checkin|sound`; the Host counts open sockets per kind so it
//! knows whether an overlay is in OBS. Sockets without a known kind (the admin window, old pages) count as other.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    Json, Router,
    extract::{
        Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::Response,
    routing::get,
};
use chat::ChatMessageEvent;

use crate::obs::{ObsStateEvent, ObsStatusEvent};
use serde::{Deserialize, Serialize};
use toolkit_core::bus::EventBus;
use utoipa::ToSchema;

/// The overlay pages that count their sockets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlayKind {
    Chat,
    Alerts,
    Checkin,
    Sound,
}

impl OverlayKind {
    /// `None` for no or an unknown `kind`; such sockets are still served.
    fn parse(kind: Option<&str>) -> Option<Self> {
        match kind? {
            "chat" => Some(Self::Chat),
            "alerts" => Some(Self::Alerts),
            "checkin" => Some(Self::Checkin),
            "sound" => Some(Self::Sound),
            _ => None,
        }
    }
}

/// Open `/ws/overlay` sockets per overlay kind.
#[derive(Default)]
pub(crate) struct OverlayConnections {
    chat: AtomicUsize,
    alerts: AtomicUsize,
    checkin: AtomicUsize,
    sound: AtomicUsize,
    other: AtomicUsize,
}

impl OverlayConnections {
    fn counter(&self, kind: Option<OverlayKind>) -> &AtomicUsize {
        match kind {
            Some(OverlayKind::Chat) => &self.chat,
            Some(OverlayKind::Alerts) => &self.alerts,
            Some(OverlayKind::Checkin) => &self.checkin,
            Some(OverlayKind::Sound) => &self.sound,
            None => &self.other,
        }
    }

    /// Counts a socket until the returned guard drops.
    fn open(self: &Arc<Self>, kind: Option<OverlayKind>) -> Open {
        self.counter(kind).fetch_add(1, Ordering::Relaxed);
        Open {
            connections: self.clone(),
            kind,
        }
    }

    fn counts(&self) -> OverlayCounts {
        let load = |c: &AtomicUsize| c.load(Ordering::Relaxed);
        OverlayCounts {
            chat: load(&self.chat),
            alerts: load(&self.alerts),
            checkin: load(&self.checkin),
            sound: load(&self.sound),
            other: load(&self.other),
        }
    }
}

struct Open {
    connections: Arc<OverlayConnections>,
    kind: Option<OverlayKind>,
}

impl Drop for Open {
    fn drop(&mut self) {
        self.connections
            .counter(self.kind)
            .fetch_sub(1, Ordering::Relaxed);
    }
}

#[derive(Clone)]
pub(crate) struct OverlayState {
    pub(crate) bus: Arc<EventBus>,
    pub(crate) connections: Arc<OverlayConnections>,
}

#[derive(Deserialize)]
pub(crate) struct WsQuery {
    kind: Option<String>,
}

pub(crate) async fn ws(
    ws: WebSocketUpgrade,
    State(s): State<OverlayState>,
    Query(query): Query<WsQuery>,
) -> Response {
    let kind = OverlayKind::parse(query.kind.as_deref());
    ws.on_upgrade(move |socket| async move {
        let _open = s.connections.open(kind);
        serve(socket, s.bus).await;
    })
}

/// Open overlay sockets per kind; `other` counts sockets without a known kind, the admin window included.
#[derive(Debug, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub(crate) struct OverlayCounts {
    chat: usize,
    alerts: usize,
    checkin: usize,
    sound: usize,
    other: usize,
}

/// Admin route; the caller adds the token check.
pub(crate) fn admin_routes(connections: Arc<OverlayConnections>) -> Router {
    Router::new()
        .route("/api/admin/overlays", get(overlay_counts))
        .with_state(connections)
}

/// How many OBS sources each overlay has open right now.
#[utoipa::path(
    get,
    path = "/api/admin/overlays",
    tag = "admin",
    responses((status = 200, body = OverlayCounts))
)]
pub(crate) async fn overlay_counts(
    State(connections): State<Arc<OverlayConnections>>,
) -> Json<OverlayCounts> {
    Json(connections.counts())
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
    /// At most one: the latest streaming-software state.
    #[serde(rename = "obs.state")]
    obs_state: Vec<ObsStateEvent>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sockets_count_per_kind_until_they_close() {
        let connections = Arc::new(OverlayConnections::default());
        let chat = connections.open(OverlayKind::parse(Some("chat")));
        let chat2 = connections.open(OverlayKind::parse(Some("chat")));
        let sound = connections.open(OverlayKind::parse(Some("sound")));
        // Old pages and the admin window send no kind; a kind this Host does not know counts the same.
        let old = connections.open(OverlayKind::parse(None));
        let unknown = connections.open(OverlayKind::parse(Some("clips")));
        assert_eq!(
            connections.counts(),
            OverlayCounts {
                chat: 2,
                alerts: 0,
                checkin: 0,
                sound: 1,
                other: 2
            }
        );
        drop((chat, sound, old, unknown));
        let counts = connections.counts();
        assert_eq!((counts.chat, counts.sound, counts.other), (1, 0, 0));
        drop(chat2);
        assert_eq!(
            connections.counts(),
            OverlayCounts {
                chat: 0,
                alerts: 0,
                checkin: 0,
                sound: 0,
                other: 0
            }
        );
    }
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
                badges: Vec::new(),
                highlighted: false,
                bits: 0,
                reply: None,
                sent_at: "2026-11-13T10:00:00Z".into(),
            },
            Source::Simulation,
        );
        crate::obs::retain(&bus);
        crate::obs::publish(&bus, integrations::obs::ObsStatus::CONNECTING);
        crate::obs::publish_state(
            &bus,
            &toolkit_core::streaming::StreamingState {
                scene: Some("Live".into()),
                scenes: vec![toolkit_core::streaming::Scene {
                    name: "Live".into(),
                    items: vec![toolkit_core::streaming::SceneItem {
                        id: 1,
                        source: "Camera".into(),
                        enabled: true,
                    }],
                }],
                inputs: vec![toolkit_core::streaming::Input {
                    name: "Mic".into(),
                    muted: false,
                }],
                streaming: true,
                recording: false,
            },
        );
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
