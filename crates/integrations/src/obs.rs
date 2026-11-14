//! OBS WebSocket (obs-websocket 5) through `obws`: connection status only, reconnecting with backoff.
//! OBS not running is a normal state: the status says so and only changes are logged.

use std::{sync::Arc, time::Duration};

use futures_util::StreamExt;
use obws::{
    Client,
    client::{ConnectConfig, HandshakeError},
    error::Error as ObwsError,
    events::Event,
    requests::EventSubscription,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

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
                    closed(&client).await;
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
            // Status only: OBS sends no events, obws still reports the connection closing.
            event_subscriptions: Some(EventSubscription::NONE),
            broadcast_capacity: 8,
            // Half the budget, so a silent host ends as `Timeout` (unreachable) before the outer limit fires.
            connect_timeout: CONNECT_TIMEOUT / 2,
        };
        match tokio::time::timeout(CONNECT_TIMEOUT, Client::connect_with_config(config)).await {
            Ok(connected) => connected.map_err(|e| classify(&e, password.is_some())),
            // Something answered the upgrade but never finished the obs-websocket handshake.
            Err(_) => Err(ObsStatus::reconnecting(Some("obs.handshake_failed"))),
        }
    }

    /// Publishes and logs only real changes, so a closed OBS never fills the log.
    fn set(&self, status: ObsStatus) {
        let changed = self.status.send_if_modified(|current| {
            let changed = *current != status;
            *current = status;
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

/// Resolves when obs-websocket closes the connection (OBS exits, the server stops or the socket drops).
async fn closed(client: &Client) {
    let Ok(mut events) = client.events() else {
        return;
    };
    while let Some(event) = events.next().await {
        if matches!(event, Event::ServerStopping | Event::ServerStopped) {
            return;
        }
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

    /// A minimal obs-websocket 5 server: handshake, `GetVersion`, and closing on demand like OBS exiting.
    #[derive(Clone, Default)]
    struct FakeObs {
        require_auth: bool,
        /// Reads `Identify` and never answers, like a stuck OBS or another WebSocket service.
        stall: bool,
        identify: Arc<Mutex<Vec<Value>>>,
        exit: Arc<Notify>,
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
                            send(&mut socket, json!({"op": 7, "d": {
                                "requestType": "GetVersion",
                                "requestId": request["d"]["requestId"],
                                "requestStatus": {"result": true, "code": 100},
                                "responseData": {
                                    "obsStudioVersion": "31.0.0", "obsWebSocketVersion": "5.5.0", "rpcVersion": 1,
                                    "availableRequests": [], "supportedImageFormats": [],
                                    "platform": "windows", "platformDescription": "test"}}}))
                            .await;
                        }
                        Some(Ok(_)) => {}
                        _ => return,
                    },
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
        let secrets = Arc::new(MemoryStore::default());
        *secrets.value.lock().unwrap() = password.map(str::to_owned);
        let (tx, rx) = watch::channel(ObsStatus::CONNECTING);
        tokio::spawn(
            Obs {
                host: "127.0.0.1".into(),
                port,
                secrets,
                status: Arc::new(tx),
            }
            .run(),
        );
        rx
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
