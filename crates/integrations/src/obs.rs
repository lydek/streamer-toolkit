//! OBS WebSocket (obs-websocket 5) through `obws`: the connection status, reconnecting with backoff, and the
//! streaming-software state (scenes, items, inputs, outputs), read in full on connect and then kept current from
//! OBS's events. OBS not running is a normal state: the status says so and only changes are logged.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use futures_util::StreamExt;
use obws::{
    Client,
    client::{ConnectConfig, HandshakeError},
    error::Error as ObwsError,
    events::Event,
    requests::{EventSubscription, inputs::InputId, scenes::SceneId},
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use toolkit_core::streaming::{Input, Scene, SceneItem, StreamingState};

use crate::secrets::SecretStore;

/// Covers the whole connect: TCP, upgrade, handshake and version check (obws times out only the first two).
// shortcut: 1 s in tests keeps the stalled-handshake test fast; make it a field if callers ever need to tune it.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(if cfg!(test) { 1 } else { 10 });
/// obs-websocket close code for a missing or wrong password.
const AUTHENTICATION_FAILED: u16 = 4009;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObsState {
    Connected,
    /// Not connected yet, or retrying on its own (e.g. OBS is closed).
    Reconnecting,
    /// Retrying, but the broadcaster has to fix something first (e.g. the password).
    Error,
}

/// What the OBS status light shows; `reason` is a stable code, never a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ObsStatus {
    pub state: ObsState,
    pub reason: Option<&'static str>,
}

impl ObsStatus {
    pub const CONNECTING: Self = Self::reconnecting(None);
    const CONNECTED: Self = Self {
        state: ObsState::Connected,
        reason: None,
    };

    const fn reconnecting(reason: Option<&'static str>) -> Self {
        Self {
            state: ObsState::Reconnecting,
            reason,
        }
    }

    const fn error(reason: &'static str) -> Self {
        Self {
            state: ObsState::Error,
            reason: Some(reason),
        }
    }
}

pub struct Obs {
    pub host: String,
    pub port: u16,
    /// The obs-websocket password; empty slot means OBS has authentication turned off.
    pub secrets: Arc<dyn SecretStore>,
    pub status: Arc<watch::Sender<ObsStatus>>,
    /// Empty (unknown scene) while not connected.
    pub state: Arc<watch::Sender<StreamingState>>,
    /// This connection's number; it writes status and state only while `current` still holds it, so a connection
    /// replaced by new settings never writes over the new one.
    pub generation: u64,
    pub current: Arc<AtomicU64>,
}

impl Obs {
    /// Runs until the task is dropped; host, port or password changes start a new task.
    pub async fn run(self) {
        self.set(ObsStatus::CONNECTING);
        let mut failures = 0u32;
        loop {
            let ended = match self.connect().await {
                Ok(client) => {
                    failures = 0;
                    self.set(ObsStatus::CONNECTED);
                    self.follow(&client).await;
                    self.set_state(&StreamingState::default());
                    ObsStatus::reconnecting(Some("obs.disconnected"))
                }
                Err(status) => status,
            };
            failures += 1;
            self.set(ended);
            tokio::time::sleep(crate::backoff(failures)).await;
        }
    }

    async fn connect(&self) -> Result<Client, ObsStatus> {
        let password = self
            .secrets
            .read()
            .map_err(|_| ObsStatus::error("obs.secret_store_failed"))?
            .map(SecretString::from);
        let config = ConnectConfig {
            host: self.host.as_str(),
            port: self.port,
            dangerous: None,
            password: password.as_ref().map(|p| p.expose_secret()),
            // obws reports the connection closing without a subscription; these keep the state current.
            event_subscriptions: Some(
                EventSubscription::SCENES
                    | EventSubscription::SCENE_ITEMS
                    | EventSubscription::INPUTS
                    | EventSubscription::OUTPUTS,
            ),
            broadcast_capacity: 64,
            // Half the budget, so a silent host ends as `Timeout` (unreachable) before the outer limit fires.
            connect_timeout: CONNECT_TIMEOUT / 2,
        };
        match tokio::time::timeout(CONNECT_TIMEOUT, Client::connect_with_config(config)).await {
            Ok(connected) => connected.map_err(|e| classify(&e, password.is_some())),
            // Something answered the upgrade but never finished the obs-websocket handshake.
            Err(_) => Err(ObsStatus::reconnecting(Some("obs.handshake_failed"))),
        }
    }

    /// Whether this connection is still the one the settings asked for.
    fn live(&self) -> bool {
        self.current.load(Ordering::SeqCst) == self.generation
    }

    /// Publishes and logs only real changes, so a closed OBS never fills the log.
    fn set(&self, status: ObsStatus) {
        // Checked inside the channel's lock, so a replaced connection cannot write after the switch.
        let changed = self.status.send_if_modified(|current| {
            let changed = self.live() && *current != status;
            if changed {
                *current = status;
            }
            changed
        });
        if changed {
            match status.state {
                ObsState::Error => tracing::warn!(reason = status.reason, "OBS connection error"),
                state => tracing::info!(?state, reason = status.reason, "OBS connection"),
            }
        }
    }
}

impl Obs {
    /// Keeps the state current until obs-websocket closes the connection (OBS exits, the server stops or the
    /// socket drops), or a read fails, which ends the connection so the next one starts fresh.
    async fn follow(&self, client: &Client) {
        // Subscribed before reading, so no change between the read and the first event is lost.
        let Ok(mut events) = client.events() else {
            return;
        };
        let Ok(mut state) = read_state(client).await else {
            return;
        };
        self.set_state(&state);
        while let Some(event) = events.next().await {
            match apply(&mut state, &event) {
                Change::Closed => return,
                Change::None => continue,
                Change::Updated => {}
                Change::Reread => match read_state(client).await {
                    Ok(fresh) => state = fresh,
                    Err(_) => return,
                },
            }
            self.set_state(&state);
        }
    }

    /// Publishes `state` when it changed and this connection is still the current one.
    fn set_state(&self, state: &StreamingState) {
        self.state.send_if_modified(|current| {
            let changed = self.live() && current != state;
            if changed {
                current.clone_from(state);
            }
            changed
        });
    }
}

/// Every scene with its items, every input with its mute state, and whether OBS streams and records.
async fn read_state(client: &Client) -> Result<StreamingState, ObwsError> {
    let list = client.scenes().list().await?;
    let mut scenes = Vec::with_capacity(list.scenes.len());
    for scene in list.scenes {
        let id = SceneId::Uuid(scene.id.uuid);
        let mut items = Vec::new();
        for item in client.scene_items().list(id).await? {
            items.push(SceneItem {
                id: item.id,
                enabled: client.scene_items().enabled(id, item.id).await?,
                source: item.source_name,
            });
        }
        scenes.push(Scene {
            name: scene.id.name,
            items,
        });
    }
    let mut inputs = Vec::new();
    for input in client.inputs().list(None).await? {
        inputs.push(Input {
            muted: client.inputs().muted(InputId::Uuid(input.id.uuid)).await?,
            name: input.id.name,
        });
    }
    Ok(StreamingState {
        scene: list.current_program_scene.map(|s| s.name),
        scenes,
        inputs,
        streaming: client.streaming().status().await?.active,
        recording: client.recording().status().await?.active,
    })
}

#[derive(Debug, PartialEq, Eq)]
enum Change {
    None,
    Updated,
    /// Scenes, items or inputs were added, removed, renamed or reordered: read everything again.
    Reread,
    Closed,
}

fn apply(state: &mut StreamingState, event: &Event) -> Change {
    match event {
        Event::ServerStopping | Event::ServerStopped => Change::Closed,
        Event::CurrentProgramSceneChanged { id } => {
            state.scene = Some(id.name.clone());
            Change::Updated
        }
        // Items inside a group are not listed, so their changes are ignored.
        Event::SceneItemEnableStateChanged {
            scene,
            item_id,
            enabled,
        } => {
            let item = state
                .scenes
                .iter_mut()
                .filter(|s| s.name == scene.name)
                .flat_map(|s| s.items.iter_mut())
                .find(|i| u64::try_from(i.id).ok() == Some(*item_id));
            match item {
                Some(item) => {
                    item.enabled = *enabled;
                    Change::Updated
                }
                None => Change::None,
            }
        }
        Event::InputMuteStateChanged { id, muted } => {
            match state.inputs.iter_mut().find(|i| i.name == id.name) {
                Some(input) => {
                    input.muted = *muted;
                    Change::Updated
                }
                None => Change::Reread,
            }
        }
        Event::StreamStateChanged { active, .. } => {
            state.streaming = *active;
            Change::Updated
        }
        Event::RecordStateChanged { active, .. } => {
            state.recording = *active;
            Change::Updated
        }
        Event::SceneListChanged { .. }
        | Event::SceneCreated { .. }
        | Event::SceneRemoved { .. }
        | Event::SceneNameChanged { .. }
        | Event::SceneItemCreated { .. }
        | Event::SceneItemRemoved { .. }
        | Event::SceneItemListReindexed { .. }
        | Event::InputCreated { .. }
        | Event::InputRemoved { .. }
        | Event::InputNameChanged { .. } => Change::Reread,
        _ => Change::None,
    }
}

fn classify(error: &ObwsError, has_password: bool) -> ObsStatus {
    match error {
        ObwsError::Handshake(HandshakeError::ConnectionClosed(Some(details)))
            if u16::from(details.code) == AUTHENTICATION_FAILED =>
        {
            ObsStatus::error(if has_password {
                "obs.auth_failed"
            } else {
                "obs.password_required"
            })
        }
        ObwsError::ObsStudioVersion(..)
        | ObwsError::ObsWebsocketVersion(..)
        | ObwsError::RpcVersion { .. } => ObsStatus::error("obs.unsupported_version"),
        ObwsError::InvalidUri(_) => ObsStatus::error("obs.invalid_address"),
        ObwsError::Connect(_) | ObwsError::Timeout => {
            ObsStatus::reconnecting(Some("obs.unreachable"))
        }
        _ => ObsStatus::reconnecting(Some("obs.handshake_failed")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::memory::MemoryStore;
    use axum::{
        Router,
        extract::{State, WebSocketUpgrade, ws},
        response::Response,
        routing::any,
    };
    use serde_json::{Value, json};
    use std::sync::Mutex;
    use tokio::sync::Notify;

    /// A minimal obs-websocket 5 server: handshake, `GetVersion`, the requests the state is read with (one scene
    /// "Live" with a camera, one microphone), events on demand, and closing on demand like OBS exiting.
    #[derive(Clone, Default)]
    struct FakeObs {
        require_auth: bool,
        /// Reads `Identify` and never answers, like a stuck OBS or another WebSocket service.
        stall: bool,
        identify: Arc<Mutex<Vec<Value>>>,
        exit: Arc<Notify>,
        /// Events waiting to be sent: `(eventType, eventData)`.
        pending: Arc<Mutex<Vec<(&'static str, Value)>>>,
        push: Arc<Notify>,
        scene_lists: Arc<Mutex<u32>>,
    }

    const LIVE: &str = "00000000-0000-0000-0000-000000000001";
    const MIC: &str = "00000000-0000-0000-0000-000000000002";

    impl FakeObs {
        fn event(&self, kind: &'static str, data: Value) {
            self.pending.lock().unwrap().push((kind, data));
            self.push.notify_one();
        }

        fn answer(&self, request_type: &str) -> Value {
            match request_type {
                "GetSceneList" => {
                    *self.scene_lists.lock().unwrap() += 1;
                    json!({"currentProgramSceneName": "Live", "currentProgramSceneUuid": LIVE,
                           "currentPreviewSceneName": null, "currentPreviewSceneUuid": null,
                           "scenes": [{"sceneName": "Live", "sceneUuid": LIVE, "sceneIndex": 0}]})
                }
                "GetSceneItemList" => json!({"sceneItems": [{
                    "sceneItemId": 1, "sceneItemIndex": 0, "sourceName": "Camera",
                    "sourceType": "OBS_SOURCE_TYPE_INPUT", "inputKind": "dshow_input", "isGroup": null}]}),
                "GetSceneItemEnabled" => json!({"sceneItemEnabled": true}),
                "GetInputList" => json!({"inputs": [{"inputName": "Mic", "inputUuid": MIC,
                    "inputKind": "wasapi_input_capture", "unversionedInputKind": "wasapi_input_capture"}]}),
                "GetInputMute" => json!({"inputMuted": false}),
                "GetStreamStatus" => json!({"outputActive": false, "outputReconnecting": false,
                    "outputTimecode": "00:00:00.000", "outputDuration": 0, "outputCongestion": 0.0,
                    "outputBytes": 0, "outputSkippedFrames": 0, "outputTotalFrames": 0}),
                "GetRecordStatus" => json!({"outputActive": true, "outputPaused": false,
                    "outputTimecode": "00:00:00.000", "outputDuration": 0, "outputBytes": 0}),
                _ => json!({
                    "obsStudioVersion": "31.0.0", "obsWebSocketVersion": "5.5.0", "rpcVersion": 1,
                    "availableRequests": [], "supportedImageFormats": [],
                    "platform": "windows", "platformDescription": "test"}),
            }
        }
    }

    async fn send(socket: &mut ws::WebSocket, value: Value) {
        let _ = socket
            .send(ws::Message::Text(value.to_string().into()))
            .await;
    }

    async fn close(socket: &mut ws::WebSocket, code: u16) {
        let frame = ws::CloseFrame {
            code,
            reason: "".into(),
        };
        let _ = socket.send(ws::Message::Close(Some(frame))).await;
    }

    async fn obs_ws(ws: WebSocketUpgrade, State(fake): State<FakeObs>) -> Response {
        ws.on_upgrade(move |mut socket| async move {
            let auth = fake
                .require_auth
                .then(|| json!({"challenge": "c", "salt": "s"}));
            send(
                &mut socket,
                json!({"op": 0, "d": {"obsWebSocketVersion": "5.5.0", "rpcVersion": 1, "authentication": auth}}),
            )
            .await;
            let Some(Ok(ws::Message::Text(identify))) = socket.recv().await else {
                return;
            };
            fake.identify
                .lock()
                .unwrap()
                .push(serde_json::from_str(identify.as_str()).unwrap());
            if fake.require_auth {
                return close(&mut socket, AUTHENTICATION_FAILED).await;
            }
            if fake.stall {
                while let Some(Ok(_)) = socket.recv().await {}
                return;
            }
            send(&mut socket, json!({"op": 2, "d": {"negotiatedRpcVersion": 1}})).await;
            loop {
                tokio::select! {
                    message = socket.recv() => match message {
                        Some(Ok(ws::Message::Text(text))) => {
                            let request: Value = serde_json::from_str(text.as_str()).unwrap();
                            let request_type = request["d"]["requestType"].as_str().unwrap_or_default();
                            send(&mut socket, json!({"op": 7, "d": {
                                "requestType": request_type,
                                "requestId": request["d"]["requestId"],
                                "requestStatus": {"result": true, "code": 100},
                                "responseData": fake.answer(request_type)}}))
                            .await;
                        }
                        Some(Ok(_)) => {}
                        _ => return,
                    },
                    _ = fake.push.notified() => {
                        let pending = std::mem::take(&mut *fake.pending.lock().unwrap());
                        for (kind, data) in pending {
                            send(&mut socket, json!({"op": 5, "d": {
                                "eventType": kind, "eventIntent": 1, "eventData": data}})).await;
                        }
                    }
                    _ = fake.exit.notified() => return close(&mut socket, 1001).await,
                }
            }
        })
    }

    async fn start(fake: FakeObs) -> u16 {
        let app = Router::new().route("/", any(obs_ws)).with_state(fake);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        port
    }

    fn run(port: u16, password: Option<&str>) -> watch::Receiver<ObsStatus> {
        run_with_state(port, password).0
    }

    fn run_with_state(
        port: u16,
        password: Option<&str>,
    ) -> (watch::Receiver<ObsStatus>, watch::Receiver<StreamingState>) {
        let secrets = Arc::new(MemoryStore::default());
        *secrets.value.lock().unwrap() = password.map(str::to_owned);
        let (tx, rx) = watch::channel(ObsStatus::CONNECTING);
        let (state_tx, state_rx) = watch::channel(StreamingState::default());
        tokio::spawn(
            Obs {
                host: "127.0.0.1".into(),
                port,
                secrets,
                status: Arc::new(tx),
                state: Arc::new(state_tx),
                generation: 1,
                current: Arc::new(AtomicU64::new(1)),
            }
            .run(),
        );
        (rx, state_rx)
    }

    #[tokio::test]
    async fn a_replaced_connection_writes_nothing() {
        let fake = FakeObs::default();
        let port = start(fake.clone()).await;
        let (tx, mut rx) = watch::channel(ObsStatus::CONNECTING);
        let (state_tx, state_rx) = watch::channel(StreamingState::default());
        // Generation 1 when the settings already moved on to 2.
        tokio::spawn(
            Obs {
                host: "127.0.0.1".into(),
                port,
                secrets: Arc::new(MemoryStore::default()),
                status: Arc::new(tx),
                state: Arc::new(state_tx),
                generation: 1,
                current: Arc::new(AtomicU64::new(2)),
            }
            .run(),
        );
        // It connects (the fake sees it identify) but publishes neither the status nor the state.
        let identified = async {
            while fake.identify.lock().unwrap().is_empty() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        };
        tokio::time::timeout(Duration::from_secs(5), identified)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(*rx.borrow_and_update(), ObsStatus::CONNECTING);
        assert_eq!(*state_rx.borrow(), StreamingState::default());
    }

    async fn state_reaches(
        rx: &mut watch::Receiver<StreamingState>,
        what: &str,
        done: impl Fn(&StreamingState) -> bool,
    ) {
        let reached = tokio::time::timeout(Duration::from_secs(5), rx.wait_for(|s| done(s)))
            .await
            .is_ok();
        assert!(reached, "{what}: state stayed {:?}", *rx.borrow());
    }

    #[tokio::test]
    async fn state_is_read_on_connect_follows_events_and_resets_on_disconnect() {
        let fake = FakeObs::default();
        let (mut status, mut state) = run_with_state(start(fake.clone()).await, None);
        reaches(&mut status, ObsStatus::CONNECTED).await;
        state_reaches(&mut state, "read on connect", |s| s.scene.is_some()).await;
        assert_eq!(
            *state.borrow(),
            StreamingState {
                scene: Some("Live".into()),
                scenes: vec![Scene {
                    name: "Live".into(),
                    items: vec![SceneItem {
                        id: 1,
                        source: "Camera".into(),
                        enabled: true,
                    }],
                }],
                inputs: vec![Input {
                    name: "Mic".into(),
                    muted: false,
                }],
                streaming: false,
                recording: true,
            }
        );

        let scene = |name: &str| json!({"sceneName": name, "sceneUuid": LIVE});
        fake.event("CurrentProgramSceneChanged", scene("BRB"));
        state_reaches(&mut state, "scene switch", |s| {
            s.scene.as_deref() == Some("BRB")
        })
        .await;
        fake.event(
            "SceneItemEnableStateChanged",
            json!({"sceneName": "Live", "sceneUuid": LIVE, "sceneItemId": 1, "sceneItemEnabled": false}),
        );
        state_reaches(&mut state, "item hidden", |s| !s.scenes[0].items[0].enabled).await;
        fake.event(
            "InputMuteStateChanged",
            json!({"inputName": "Mic", "inputUuid": MIC, "inputMuted": true}),
        );
        state_reaches(&mut state, "input muted", |s| s.inputs[0].muted).await;
        fake.event(
            "StreamStateChanged",
            json!({"outputActive": true, "outputState": "OBS_WEBSOCKET_OUTPUT_STARTED"}),
        );
        state_reaches(&mut state, "streaming", |s| s.streaming).await;

        // A structural change reads everything again (the fake's state is back to its start).
        let lists = *fake.scene_lists.lock().unwrap();
        fake.event(
            "SceneCreated",
            json!({"sceneName": "New", "sceneUuid": LIVE, "isGroup": false}),
        );
        state_reaches(&mut state, "read again", |s| {
            s.scene.as_deref() == Some("Live")
        })
        .await;
        assert_eq!(*fake.scene_lists.lock().unwrap(), lists + 1);

        fake.exit.notify_one();
        state_reaches(&mut state, "reset on disconnect", |s| {
            *s == StreamingState::default()
        })
        .await;
        // Connecting again reads the state again.
        state_reaches(&mut state, "read on reconnect", |s| s.scene.is_some()).await;
    }

    async fn reaches(rx: &mut watch::Receiver<ObsStatus>, expected: ObsStatus) {
        let reached = tokio::time::timeout(Duration::from_secs(5), rx.wait_for(|s| *s == expected))
            .await
            .is_ok();
        assert!(
            reached,
            "status stayed {:?}, expected {expected:?}",
            *rx.borrow()
        );
    }

    #[tokio::test]
    async fn status_follows_obs_closing_and_coming_back() {
        let fake = FakeObs::default();
        let mut rx = run(start(fake.clone()).await, None);
        reaches(&mut rx, ObsStatus::CONNECTED).await;

        fake.exit.notify_one();
        reaches(&mut rx, ObsStatus::reconnecting(Some("obs.disconnected"))).await;
        // The first retry comes after 1 s.
        reaches(&mut rx, ObsStatus::CONNECTED).await;
        assert_eq!(fake.identify.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn stalled_handshake_times_out_and_retries() {
        let fake = FakeObs {
            stall: true,
            ..FakeObs::default()
        };
        let mut rx = run(start(fake.clone()).await, None);
        reaches(
            &mut rx,
            ObsStatus::reconnecting(Some("obs.handshake_failed")),
        )
        .await;
        // The 1 s backoff, then another attempt that stalls again.
        let retried = async {
            while fake.identify.lock().unwrap().len() < 2 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        };
        assert!(
            tokio::time::timeout(Duration::from_secs(5), retried)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn closed_obs_is_reported_as_reconnecting() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let mut rx = run(port, None);
        reaches(&mut rx, ObsStatus::reconnecting(Some("obs.unreachable"))).await;
    }

    #[tokio::test]
    async fn wrong_password_is_an_error_with_its_reason() {
        let fake = FakeObs {
            require_auth: true,
            ..FakeObs::default()
        };
        let mut rx = run(start(fake.clone()).await, Some("hunter2"));
        reaches(&mut rx, ObsStatus::error("obs.auth_failed")).await;
        let identify = fake.identify.lock().unwrap()[0].to_string();
        assert!(identify.contains("authentication"));
        assert!(
            !identify.contains("hunter2"),
            "only the challenge response is sent"
        );
    }

    #[tokio::test]
    async fn missing_password_is_its_own_reason() {
        let fake = FakeObs {
            require_auth: true,
            ..FakeObs::default()
        };
        let mut rx = run(start(fake).await, None);
        reaches(&mut rx, ObsStatus::error("obs.password_required")).await;
    }

    #[test]
    fn timeout_means_unreachable() {
        assert_eq!(
            classify(&ObwsError::Timeout, true),
            ObsStatus::reconnecting(Some("obs.unreachable"))
        );
    }

    #[test]
    fn status_serializes_for_the_status_light() {
        assert_eq!(
            serde_json::to_value(ObsStatus::error("obs.auth_failed")).unwrap(),
            json!({"state": "error", "reason": "obs.auth_failed"})
        );
    }
}
