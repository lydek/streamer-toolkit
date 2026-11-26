//! Read-only overlay channel: the first WebSocket message is a snapshot, then events follow in `seq` order.
//! Overlay pages connect with `?kind=chat|alerts|checkin|sound|bgm|counter|marathon|break|warnings|leaderboard`; the Host counts open sockets per kind so it
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
use chat::{ChatMessageEvent, ChatRemovedEvent};
use tokio::sync::watch;

use crate::{
    obs::{ObsStateEvent, ObsStatusEvent},
    queue::PresentationQueueEvent,
};
use serde::{Deserialize, Serialize};
use toolkit_core::{bus::EventBus, presentation::Stage};
use utoipa::ToSchema;

/// The overlay pages that count their sockets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlayKind {
    Chat,
    Alerts,
    Checkin,
    Sound,
    Bgm,
    Counter,
    Marathon,
    Break,
    Warnings,
    Leaderboard,
}

impl OverlayKind {
    /// `None` for no or an unknown `kind`; such sockets are still served.
    fn parse(kind: Option<&str>) -> Option<Self> {
        match kind? {
            "chat" => Some(Self::Chat),
            "alerts" => Some(Self::Alerts),
            "checkin" => Some(Self::Checkin),
            "sound" => Some(Self::Sound),
            "bgm" => Some(Self::Bgm),
            "counter" => Some(Self::Counter),
            "marathon" => Some(Self::Marathon),
            "break" => Some(Self::Break),
            "warnings" => Some(Self::Warnings),
            "leaderboard" => Some(Self::Leaderboard),
            _ => None,
        }
    }
}

impl From<Stage> for OverlayKind {
    fn from(stage: Stage) -> Self {
        match stage {
            Stage::Alerts => Self::Alerts,
            Stage::Checkin => Self::Checkin,
            Stage::Sound => Self::Sound,
        }
    }
}

/// Open `/ws/overlay` sockets per overlay kind.
pub(crate) struct OverlayConnections {
    chat: AtomicUsize,
    alerts: AtomicUsize,
    checkin: AtomicUsize,
    sound: AtomicUsize,
    bgm: AtomicUsize,
    counter: AtomicUsize,
    marathon: AtomicUsize,
    break_card: AtomicUsize,
    warnings: AtomicUsize,
    leaderboard: AtomicUsize,
    other: AtomicUsize,
    /// Ticks whenever a socket opens or closes.
    changes: watch::Sender<()>,
}

impl Default for OverlayConnections {
    fn default() -> Self {
        Self {
            chat: AtomicUsize::default(),
            alerts: AtomicUsize::default(),
            checkin: AtomicUsize::default(),
            sound: AtomicUsize::default(),
            bgm: AtomicUsize::default(),
            counter: AtomicUsize::default(),
            marathon: AtomicUsize::default(),
            break_card: AtomicUsize::default(),
            warnings: AtomicUsize::default(),
            leaderboard: AtomicUsize::default(),
            other: AtomicUsize::default(),
            changes: watch::channel(()).0,
        }
    }
}

impl OverlayConnections {
    fn counter(&self, kind: Option<OverlayKind>) -> &AtomicUsize {
        match kind {
            Some(OverlayKind::Chat) => &self.chat,
            Some(OverlayKind::Alerts) => &self.alerts,
            Some(OverlayKind::Checkin) => &self.checkin,
            Some(OverlayKind::Sound) => &self.sound,
            Some(OverlayKind::Bgm) => &self.bgm,
            Some(OverlayKind::Counter) => &self.counter,
            Some(OverlayKind::Marathon) => &self.marathon,
            Some(OverlayKind::Break) => &self.break_card,
            Some(OverlayKind::Warnings) => &self.warnings,
            Some(OverlayKind::Leaderboard) => &self.leaderboard,
            None => &self.other,
        }
    }

    /// How many sockets of `kind` are open; the presentation queue skips an overlay with none, and the music clock
    /// stands still without one.
    pub(crate) fn count(&self, kind: OverlayKind) -> usize {
        self.counter(Some(kind)).load(Ordering::Relaxed)
    }

    /// Follows sockets opening and closing (the presentation queue shows which overlays are open).
    pub(crate) fn changes(&self) -> watch::Receiver<()> {
        self.changes.subscribe()
    }

    /// Counts a socket until the returned guard drops.
    fn open(self: &Arc<Self>, kind: Option<OverlayKind>) -> Open {
        self.counter(kind).fetch_add(1, Ordering::Relaxed);
        self.changes.send_replace(());
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
            bgm: load(&self.bgm),
            counter: load(&self.counter),
            marathon: load(&self.marathon),
            break_card: load(&self.break_card),
            warnings: load(&self.warnings),
            leaderboard: load(&self.leaderboard),
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
        self.connections.changes.send_replace(());
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
    bgm: usize,
    counter: usize,
    marathon: usize,
    #[serde(rename = "break")]
    break_card: usize,
    warnings: usize,
    leaderboard: usize,
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
    /// The latest removals, oldest first: what was taken down before this snapshot.
    #[serde(rename = "chat.removed")]
    chat_removed: Vec<ChatRemovedEvent>,
    /// At most one: the latest status.
    #[serde(rename = "obs.status")]
    obs_status: Vec<ObsStatusEvent>,
    /// At most one: the latest streaming-software state.
    #[serde(rename = "obs.state")]
    obs_state: Vec<ObsStateEvent>,
    /// At most one: the latest presentation queue status.
    #[serde(rename = "presentation.queue")]
    presentation_queue: Vec<PresentationQueueEvent>,
    /// At most one: the latest live state.
    #[serde(rename = "stream.status")]
    stream_status: Vec<stream::StreamStatusEvent>,
    /// At most one: the latest background music player status.
    #[serde(rename = "bgm.player")]
    bgm_player: Vec<bgm::BgmPlayerEvent>,
    /// At most one: every counter's latest value.
    #[serde(rename = "counter.values")]
    counter_values: Vec<counter::CounterValuesEvent>,
    /// At most one: the latest sub marathon timer.
    #[serde(rename = "counter.marathon")]
    counter_marathon: Vec<counter::MarathonEvent>,
    /// At most one: the break card.
    #[serde(rename = "screen.break")]
    screen_break: Vec<overlays::BreakEvent>,
    /// At most one: the on-stream warnings.
    #[serde(rename = "screen.warnings")]
    screen_warnings: Vec<overlays::WarningsEvent>,
    /// At most one: the built-in overlays' theme and text size.
    #[serde(rename = "screen.appearance")]
    screen_appearance: Vec<overlays::AppearanceEvent>,
    /// At most one: the latest leaderboard.
    #[serde(rename = "leaderboard.update")]
    leaderboard_update: Vec<crate::leaderboard::LeaderboardEvent>,
    /// At most one: the chat overlay's text size.
    #[serde(rename = "chat.text_size")]
    chat_text_size: Vec<crate::templates::ChatTextSizeEvent>,
    /// At most one: the latest poll.
    #[serde(rename = "twitch.contest")]
    twitch_contest: Vec<crate::contests::ContestEvent>,
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
        let bgm = connections.open(OverlayKind::parse(Some("bgm")));
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
                bgm: 1,
                counter: 0,
                marathon: 0,
                break_card: 0,
                warnings: 0,
                leaderboard: 0,
                other: 2
            }
        );
        drop((chat, sound, bgm, old, unknown));
        let counts = connections.counts();
        assert_eq!(
            (counts.chat, counts.sound, counts.bgm, counts.other),
            (1, 0, 0, 0)
        );
        drop(chat2);
        assert_eq!(
            connections.counts(),
            OverlayCounts {
                chat: 0,
                alerts: 0,
                checkin: 0,
                sound: 0,
                bgm: 0,
                counter: 0,
                marathon: 0,
                break_card: 0,
                warnings: 0,
                leaderboard: 0,
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
                broadcaster_id: String::new(),
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
                twitch_badges: Vec::new(),
                shared_from: None,
                sent_at: "2026-11-13T10:00:00Z".into(),
            },
            Source::Simulation,
        );
        crate::obs::retain(&bus);
        crate::obs::publish(&bus, integrations::obs::ObsStatus::CONNECTING);
        bus.retain("stream.status", 1);
        bus.retain("bgm.player", 1);
        bus.retain("counter.values", 1);
        bus.retain("counter.marathon", 1);
        bus.retain("chat.text_size", 1);
        bus.retain("screen.break", 1);
        bus.retain("screen.warnings", 1);
        bus.retain("screen.appearance", 1);
        bus.retain("leaderboard.update", 1);
        bus.retain("twitch.contest", 1);
        bus.publish(toolkit_core::bus::Event {
            id: "v".into(),
            event_type: "twitch.contest",
            version: 1,
            occurred_at: "2026-11-27T10:00:00.000Z".into(),
            source: Source::Twitch,
            payload: serde_json::json!({ "poll": {
                "id": "p1", "title": "下一首？", "status": "active", "endsAt": "2026-11-27T10:02:00Z",
                "choices": [{ "id": "c1", "title": "晴天", "votes": 3 }]
            }}),
        });
        bus.publish(toolkit_core::bus::Event {
            id: "l".into(),
            event_type: "leaderboard.update",
            version: 1,
            occurred_at: "2026-11-22T10:00:00.000Z".into(),
            source: Source::System,
            payload: serde_json::json!({ "boards": [
                { "board": "bits", "period": "stream", "entries": [{ "name": "小魚", "value": 500 }] }
            ]}),
        });
        bus.publish(toolkit_core::bus::Event {
            id: "a".into(),
            event_type: "screen.appearance",
            version: 1,
            occurred_at: "2026-11-22T10:00:00.000Z".into(),
            source: Source::System,
            payload: serde_json::json!({ "theme": "cream", "textSize": 120 }),
        });
        bus.publish(toolkit_core::bus::Event {
            id: "w".into(),
            event_type: "screen.warnings",
            version: 1,
            occurred_at: "2026-11-22T10:00:00.000Z".into(),
            source: Source::System,
            payload: serde_json::json!({
                "chatOverlay": true, "twitch": false, "mic": true, "micSeconds": 30, "micInput": "Mic",
                "chatOverlayDown": true, "twitchDown": false, "micMutedSince": "2026-11-22T10:00:00.000Z",
                "micMissing": false
            }),
        });
        bus.publish(toolkit_core::bus::Event {
            id: "k".into(),
            event_type: "screen.break",
            version: 1,
            occurred_at: "2026-11-22T10:00:00.000Z".into(),
            source: Source::System,
            payload: serde_json::json!({
                "shown": true, "text": "休息", "endText": "馬上回來", "endsAt": "2026-11-22T10:10:00.000Z",
                "mode": "minutes", "minutes": 10, "untilTime": "20:00"
            }),
        });
        bus.publish(toolkit_core::bus::Event {
            id: "t".into(),
            event_type: "chat.text_size",
            version: 1,
            occurred_at: "2026-11-21T10:00:00.000Z".into(),
            source: Source::System,
            payload: serde_json::json!({ "textSize": 32 }),
        });
        bus.publish(toolkit_core::bus::Event {
            id: "m".into(),
            event_type: "counter.marathon",
            version: 1,
            occurred_at: "2026-11-21T10:00:00.000Z".into(),
            source: Source::System,
            payload: serde_json::json!({
                "remainingMs": 3_600_000, "running": true, "paused": false, "live": true,
                "subMinutes": 5, "bitsMinutes": 2, "maxHours": 24
            }),
        });
        bus.publish(toolkit_core::bus::Event {
            id: "c".into(),
            event_type: "counter.values",
            version: 1,
            occurred_at: "2026-11-21T10:00:00.000Z".into(),
            source: Source::System,
            payload: serde_json::json!({ "counters": [{ "id": "k", "name": "死亡次數", "value": 3 }] }),
        });
        bus.publish(toolkit_core::bus::Event {
            id: "b".into(),
            event_type: "bgm.player",
            version: 1,
            occurred_at: "2026-11-21T10:00:00.000Z".into(),
            source: Source::System,
            payload: serde_json::json!({
                "state": "playing", "playlist": "p", "positionMs": 1200, "onAir": true, "shuffle": false,
                "volume": 50, "track": {"id": "t", "title": "Lo-fi", "url": "/music/t", "durationMs": 180000}
            }),
        });
        bus.publish(toolkit_core::bus::Event {
            id: "s".into(),
            event_type: "stream.status",
            version: 1,
            occurred_at: "2026-11-19T10:00:00.000Z".into(),
            source: Source::Twitch,
            payload: serde_json::json!({"state": "live", "startedAt": "2026-11-19T10:00:00.000Z", "viewers": 42}),
        });
        crate::queue::retain(&bus);
        crate::queue::publish_status(
            &bus,
            &toolkit_core::presentation::QueueStatus {
                current: Some(toolkit_core::presentation::Playing {
                    stage: toolkit_core::presentation::Stage::Alerts,
                    subject: toolkit_core::presentation::Subject::Alert {
                        alert: "follow".into(),
                        viewer: Some("Fan".into()),
                    },
                    show_ms: 6000,
                    started_at: "2026-11-19T10:00:00.000Z".into(),
                    problem: Some(toolkit_core::presentation::Problem::SoundMissing),
                }),
                waiting: vec![toolkit_core::presentation::Waiting {
                    stage: toolkit_core::presentation::Stage::Sound,
                    subject: toolkit_core::presentation::Subject::Sound {
                        sound: "Bell".into(),
                        viewer: "Kai".into(),
                    },
                    show_ms: 1500,
                    on_air: false,
                }],
                gap_ms: 1000,
            },
        );
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
                        locked: false,
                        kind: "dshow_input".into(),
                        children: vec![],
                    }],
                }],
                inputs: vec![toolkit_core::streaming::Input {
                    name: "Mic".into(),
                    muted: false,
                    ..Default::default()
                }],
                streaming: true,
                recording: false,
                studio_mode: true,
                preview: Some("Live".into()),
                transitions: vec!["Fade".into()],
                transition: Some("Fade".into()),
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
        let music = &wire["payload"]["bgm.player"][0];
        assert_required("BgmPlayerEvent", music);
        assert_required("BgmPlayerPayload", &music["payload"]);
        assert_required("PlayerTrack", &music["payload"]["track"]);
        let counters = &wire["payload"]["counter.values"][0];
        assert_required("CounterValuesEvent", counters);
        assert_required("CounterValue", &counters["payload"]["counters"][0]);
        let marathon = &wire["payload"]["counter.marathon"][0];
        assert_required("MarathonEvent", marathon);
        assert_required("MarathonPayload", &marathon["payload"]);
        let card = &wire["payload"]["screen.break"][0];
        assert_required("BreakEvent", card);
        assert_required("BreakPayload", &card["payload"]);
        let warnings = &wire["payload"]["screen.warnings"][0];
        assert_required("WarningsEvent", warnings);
        assert_required("WarningsPayload", &warnings["payload"]);
        let appearance = &wire["payload"]["screen.appearance"][0];
        assert_required("AppearanceEvent", appearance);
        assert_required("AppearancePayload", &appearance["payload"]);
        let board = &wire["payload"]["leaderboard.update"][0];
        assert_required("LeaderboardEvent", board);
        assert_required("LeaderboardPayload", &board["payload"]);
        assert_required("Board", &board["payload"]["boards"][0]);
        assert_required("BoardEntry", &board["payload"]["boards"][0]["entries"][0]);
        let size = &wire["payload"]["chat.text_size"][0];
        assert_required("ChatTextSizeEvent", size);
        assert_required("ChatTextSizePayload", &size["payload"]);
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
