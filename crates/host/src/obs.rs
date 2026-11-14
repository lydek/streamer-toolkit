//! OBS connection: runs the `integrations` adapter, publishes its status as `obs.status` (kept in the WebSocket
//! snapshot) and lets the admin API change host, port and password. Admin routes; the caller adds the token check.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use integrations::{
    obs::{Obs, ObsStatus},
    secrets::SecretStore,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tokio::{sync::watch, task::AbortHandle};
use toolkit_core::{
    bus::{Event, EventBus},
    events::Source,
    settings,
};

pub(crate) const STATUS_EVENT: &str = "obs.status";

pub(crate) struct ObsLink {
    data_dir: PathBuf,
    secrets: Arc<dyn SecretStore>,
    status: Arc<watch::Sender<ObsStatus>>,
    task: Mutex<Option<AbortHandle>>,
}

impl ObsLink {
    /// Connects with the saved settings and forwards every status change to the bus.
    pub(crate) fn start(
        data_dir: PathBuf,
        secrets: Arc<dyn SecretStore>,
        bus: Arc<EventBus>,
    ) -> Arc<Self> {
        let status = Arc::new(watch::channel(ObsStatus::CONNECTING).0);
        bus.retain(STATUS_EVENT, 1);
        let mut changes = status.subscribe();
        tokio::spawn(async move {
            loop {
                let current = *changes.borrow_and_update();
                bus.publish(Event {
                    id: uuid::Uuid::now_v7().to_string(),
                    event_type: STATUS_EVENT,
                    version: 1,
                    occurred_at: chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                    source: Source::Obs,
                    payload: serde_json::to_value(current).expect("status serializes"),
                });
                if changes.changed().await.is_err() {
                    return;
                }
            }
        });
        let link = Arc::new(Self {
            data_dir,
            secrets,
            status,
            task: Mutex::new(None),
        });
        link.restart();
        link
    }

    /// Starts a connection with the saved settings and stops the previous one.
    fn restart(&self) {
        let saved = settings::load(&self.data_dir).obs;
        let task = tokio::spawn(
            Obs {
                host: saved.host,
                port: saved.port,
                secrets: self.secrets.clone(),
                status: self.status.clone(),
            }
            .run(),
        );
        if let Some(old) = self.task.lock().unwrap().replace(task.abort_handle()) {
            old.abort();
        }
    }
}

pub(crate) fn routes(link: Arc<ObsLink>) -> Router {
    Router::new()
        .route("/api/admin/obs", get(overview).put(update))
        .with_state(link)
}

#[derive(Serialize)]
struct Overview {
    host: String,
    port: u16,
    password_set: bool,
    status: ObsStatus,
}

async fn overview(State(link): State<Arc<ObsLink>>) -> Response {
    let Ok(password) = link.secrets.read() else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "obs.secret_store_failed");
    };
    let saved = settings::load(&link.data_dir).obs;
    Json(Overview {
        host: saved.host,
        port: saved.port,
        password_set: password.is_some(),
        status: *link.status.borrow(),
    })
    .into_response()
}

#[derive(Deserialize)]
struct UpdateBody {
    host: String,
    port: u16,
    /// Absent keeps the stored password; empty removes it (OBS with authentication off).
    #[serde(default)]
    password: Option<SecretString>,
}

/// Host names and IPv4 addresses only, so the value is safe inside `ws://<host>:<port>`.
fn valid_host(host: &str) -> bool {
    (1..=253).contains(&host.len())
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
}

async fn update(State(link): State<Arc<ObsLink>>, Json(body): Json<UpdateBody>) -> Response {
    let host = body.host.trim().to_owned();
    if !valid_host(&host) || body.port == 0 {
        return error(StatusCode::BAD_REQUEST, "obs.invalid_address");
    }
    if let Some(password) = &body.password {
        let saved = match password.expose_secret() {
            "" => link.secrets.delete(),
            value => link.secrets.write(value),
        };
        if saved.is_err() {
            return error(StatusCode::INTERNAL_SERVER_ERROR, "obs.secret_store_failed");
        }
    }
    if settings::update(&link.data_dir, |s| {
        s.obs.host = host;
        s.obs.port = body.port;
    })
    .is_err()
    {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed");
    }
    link.restart();
    StatusCode::NO_CONTENT.into_response()
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(serde_json::json!({ "code": code }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use integrations::secrets::SecretStoreError;
    use std::time::Duration;

    struct NoPassword;
    impl SecretStore for NoPassword {
        fn read(&self) -> Result<Option<String>, SecretStoreError> {
            Ok(None)
        }
        fn write(&self, _: &str) -> Result<(), SecretStoreError> {
            Ok(())
        }
        fn delete(&self) -> Result<(), SecretStoreError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn status_reaches_the_websocket_snapshot() {
        let dir = std::env::temp_dir().join(format!("st-obs-snapshot-{}", std::process::id()));
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        settings::update(&dir, |s| s.obs.port = port).unwrap();
        let bus = Arc::new(EventBus::default());
        let _link = ObsLink::start(dir.clone(), Arc::new(NoPassword), bus.clone());

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let (snapshot, _) = bus.subscribe();
            let snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
            let status = &snapshot["payload"][STATUS_EVENT][0];
            if status["payload"]["reason"] == "obs.unreachable" {
                assert_eq!(status["payload"]["state"], "reconnecting");
                assert_eq!(status["source"], "obs");
                assert_eq!(
                    snapshot["payload"][STATUS_EVENT].as_array().unwrap().len(),
                    1
                );
                break;
            }
            assert!(tokio::time::Instant::now() < deadline, "{snapshot}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn host_validation() {
        assert!(valid_host("127.0.0.1"));
        assert!(valid_host("obs-pc.local"));
        assert!(!valid_host(""));
        assert!(!valid_host("evil.example/path"));
        assert!(!valid_host("user@host"));
        assert!(!valid_host("host:1234"));
        assert!(!valid_host(&"a".repeat(254)));
    }
}
