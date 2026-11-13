//! Local Host: HTTP + WebSocket bound to 127.0.0.1 (see docs/architecture.md §5).

mod admin;
mod overlay;
pub mod settings;

use std::{
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
};

use integrations::secrets::SecretStore;
use secrecy::SecretString;

use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, Uri, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use rust_embed::Embed;
use tokio::net::TcpListener;

/// Default Host port; OBS Browser Source URLs depend on it staying stable.
pub const DEFAULT_PORT: u16 = 47810;

// frame-src 'none': the release IPC capability covers the whole Host origin, so no same-origin page may be framed
// inside the admin window.
const ADMIN_CSP: &str = "script-src 'self'; connect-src 'self' ipc: http://ipc.localhost; \
style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; frame-src 'none'";
const OVERLAY_CSP: &str = "script-src 'self'; object-src 'none'";

#[derive(Embed)]
#[folder = "../../apps/web/dist"]
struct Web;

pub struct HostConfig {
    pub port: u16,
    /// Extra allowed origins, e.g. the Vite dev server in dev mode (decided by apps/desktop).
    pub extra_origins: Vec<String>,
    /// Per-launch random token required by `/api/admin`; never written to disk.
    pub admin_token: SecretString,
    /// App config directory holding `settings.json`.
    pub config_dir: PathBuf,
    /// Where the broadcaster's Twitch tokens live (Windows Credential Manager in production).
    pub twitch_secrets: Arc<dyn SecretStore>,
    pub twitch: TwitchEndpoints,
}

/// Twitch addresses; tests point them at fake services.
#[derive(Clone)]
pub struct TwitchEndpoints {
    pub oauth: String,
    pub helix: String,
    pub eventsub: String,
}

impl Default for TwitchEndpoints {
    fn default() -> Self {
        use integrations::twitch::{auth, eventsub, helix};
        Self {
            oauth: auth::OAUTH_BASE.into(),
            helix: helix::HELIX_BASE.into(),
            eventsub: eventsub::EVENTSUB_URL.into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("port {0} is already in use")]
    PortInUse(u16),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub struct BoundHost {
    listener: TcpListener,
    router: Router,
}

/// Binds before serving so the caller can open the port-change page when the port is taken.
pub async fn bind(config: HostConfig) -> Result<BoundHost, HostError> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, config.port));
    let listener = TcpListener::bind(addr).await.map_err(|e| match e.kind() {
        std::io::ErrorKind::AddrInUse => HostError::PortInUse(config.port),
        _ => HostError::Io(e),
    })?;
    Ok(BoundHost {
        listener,
        router: router(config)?,
    })
}

impl BoundHost {
    pub async fn serve(self) -> std::io::Result<()> {
        axum::serve(self.listener, self.router).await
    }
}

#[derive(Clone)]
struct Guard {
    hosts: Vec<String>,
    origins: Vec<String>,
}

impl Guard {
    fn new(config: &HostConfig) -> Self {
        let hosts = vec![
            format!("127.0.0.1:{}", config.port),
            format!("localhost:{}", config.port),
        ];
        let mut origins: Vec<String> = hosts.iter().map(|h| format!("http://{h}")).collect();
        origins.extend(config.extra_origins.iter().cloned());
        Self { hosts, origins }
    }
}

pub fn router(config: HostConfig) -> Result<Router, HostError> {
    let guard_state = Guard::new(&config);
    let tls = integrations::tls::client_config().map_err(std::io::Error::other)?;
    let hub = Arc::new(overlay::OverlayHub::new());
    let admin = admin::AdminState::new(
        config.admin_token,
        config.config_dir,
        config.twitch_secrets,
        config.twitch,
        tls,
        hub.clone(),
    );
    Ok(Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .merge(
            Router::new()
                .route("/ws/overlay", get(overlay::ws))
                .with_state(hub),
        )
        .merge(admin::routes(Arc::new(admin)))
        .fallback(static_file)
        .layer(middleware::from_fn_with_state(guard_state, guard)))
}

/// Blocks DNS rebinding (Host header) and cross-site WebSocket / state-changing requests (Origin).
async fn guard(State(g): State<Guard>, req: Request, next: Next) -> Response {
    // Scoped so the borrow of `req` ends before `.await` (keeps the future `Send`).
    let allowed = {
        let header_str = |name| {
            req.headers()
                .get(name)
                .and_then(|v: &HeaderValue| v.to_str().ok())
        };
        let host_ok = header_str(header::HOST).is_some_and(|h| g.hosts.iter().any(|a| a == h));
        let is_ws =
            header_str(header::UPGRADE).is_some_and(|u| u.eq_ignore_ascii_case("websocket"));
        let changes_state = !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
        let origin_ok =
            header_str(header::ORIGIN).is_some_and(|o| g.origins.iter().any(|a| a == o));
        host_ok && (origin_ok || !(is_ws || changes_state))
    };
    if !allowed {
        return StatusCode::FORBIDDEN.into_response();
    }
    next.run(req).await
}

async fn static_file(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/').trim_end_matches('/');
    let candidates = [path.to_owned(), format!("{path}/index.html")];
    let found = candidates
        .iter()
        .find_map(|p| Web::get(p).map(|f| (p.clone(), f)));
    // The admin UI is an SPA: unknown /admin/* paths fall back to its index.
    let found = found.or_else(|| {
        (path == "admin" || path.starts_with("admin/"))
            .then(|| Web::get("admin/index.html").map(|f| ("admin/index.html".to_owned(), f)))
            .flatten()
    });
    let Some((file_path, file)) = found else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let mime = mime_guess::from_path(&file_path).first_or_octet_stream();
    let mut res = Response::new(Body::from(file.data.into_owned()));
    let headers = res.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.as_ref()).unwrap(),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if file_path.starts_with("admin/") {
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(ADMIN_CSP),
        );
    } else if file_path.starts_with("overlay/") {
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(OVERLAY_CSP),
        );
    } else if file_path.starts_with("assets/") {
        // Shared public chunks; the sandboxed (null-origin) preview iframe loads them via CORS.
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        );
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use integrations::secrets::SecretStoreError;
    use tower::ServiceExt;

    const TOKEN: &str = "test-admin-token";

    struct NoSecrets;
    impl SecretStore for NoSecrets {
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

    fn app() -> Router {
        router(HostConfig {
            port: 47810,
            extra_origins: vec!["http://localhost:5173".into()],
            admin_token: TOKEN.into(),
            config_dir: std::env::temp_dir().join(format!("st-host-test-{}", std::process::id())),
            twitch_secrets: Arc::new(NoSecrets),
            twitch: TwitchEndpoints {
                oauth: "http://127.0.0.1:1".into(),
                helix: "http://127.0.0.1:1".into(),
                eventsub: "ws://127.0.0.1:1".into(),
            },
        })
        .unwrap()
    }

    #[tokio::test]
    async fn admin_api_requires_the_token() {
        let no_token = get_req("/api/admin/twitch", "127.0.0.1:47810")
            .body(Body::empty())
            .unwrap();
        assert_eq!(status(no_token).await, StatusCode::UNAUTHORIZED);

        let wrong = get_req("/api/admin/twitch", "127.0.0.1:47810")
            .header(header::AUTHORIZATION, "Bearer nope")
            .body(Body::empty())
            .unwrap();
        assert_eq!(status(wrong).await, StatusCode::UNAUTHORIZED);

        let ok = get_req("/api/admin/twitch", "127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(status(ok).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn invalid_client_id_is_rejected() {
        let req = get_req("/api/admin/twitch/client-id", "127.0.0.1:47810")
            .method(Method::PUT)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"client_id":"<script>"}"#))
            .unwrap();
        assert_eq!(status(req).await, StatusCode::BAD_REQUEST);
    }

    /// One secret slot that remembers deletes, for tests that need a stored Twitch token.
    #[derive(Default)]
    struct MemSecrets(std::sync::Mutex<Option<String>>);
    impl SecretStore for MemSecrets {
        fn read(&self) -> Result<Option<String>, SecretStoreError> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn write(&self, v: &str) -> Result<(), SecretStoreError> {
            *self.0.lock().unwrap() = Some(v.into());
            Ok(())
        }
        fn delete(&self) -> Result<(), SecretStoreError> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }

    fn put_client_id(id: &str) -> Request {
        get_req("/api/admin/twitch/client-id", "127.0.0.1:47810")
            .method(Method::PUT)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(format!(r#"{{"client_id":"{id}"}}"#)))
            .unwrap()
    }

    #[tokio::test]
    async fn saving_the_same_client_id_keeps_the_sign_in() {
        let dir = std::env::temp_dir().join(format!("st-host-same-id-{}", std::process::id()));
        settings::update(&dir, |s| s.twitch_client_id = "abc".into()).unwrap();
        let secrets = Arc::new(MemSecrets::default());
        *secrets.0.lock().unwrap() = Some(
            r#"{"client_id":"abc","access_token":"a","refresh_token":"r","user_id":"42","login":"streamer","scopes":[]}"#.into(),
        );
        let app = router(HostConfig {
            port: 47810,
            extra_origins: Vec::new(),
            admin_token: TOKEN.into(),
            config_dir: dir.clone(),
            twitch_secrets: secrets.clone(),
            twitch: TwitchEndpoints {
                oauth: "http://127.0.0.1:1".into(),
                helix: "http://127.0.0.1:1".into(),
                eventsub: "ws://127.0.0.1:1".into(),
            },
        })
        .unwrap();

        let res = app.clone().oneshot(put_client_id("abc")).await.unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert!(
            secrets.0.lock().unwrap().is_some(),
            "unchanged ID keeps the token"
        );

        let res = app.clone().oneshot(put_client_id("xyz")).await.unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert!(secrets.0.lock().unwrap().is_none(), "a new ID signs out");
        assert_eq!(settings::load(&dir).twitch_client_id, "xyz");
        let _ = std::fs::remove_dir_all(dir);
    }

    fn get_req(uri: &str, host: &str) -> axum::http::request::Builder {
        Request::builder().uri(uri).header(header::HOST, host)
    }

    async fn status(req: Request) -> StatusCode {
        app().oneshot(req).await.unwrap().status()
    }

    fn ws_req(origin: Option<&str>) -> Request {
        let mut b = get_req("/ws/overlay", "127.0.0.1:47810")
            .header(header::UPGRADE, "websocket")
            .header(header::CONNECTION, "upgrade")
            .header(header::SEC_WEBSOCKET_VERSION, "13")
            .header(header::SEC_WEBSOCKET_KEY, "dGhlIHNhbXBsZSBub25jZQ==");
        if let Some(o) = origin {
            b = b.header(header::ORIGIN, o);
        }
        b.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn rejects_foreign_host_header() {
        let req = get_req("/api/health", "evil.example:47810")
            .body(Body::empty())
            .unwrap();
        assert_eq!(status(req).await, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn accepts_loopback_host_header() {
        for host in ["127.0.0.1:47810", "localhost:47810"] {
            let req = get_req("/api/health", host).body(Body::empty()).unwrap();
            assert_eq!(status(req).await, StatusCode::OK, "{host}");
        }
    }

    #[tokio::test]
    async fn websocket_requires_allowed_origin() {
        assert_eq!(status(ws_req(None)).await, StatusCode::FORBIDDEN);
        assert_eq!(status(ws_req(Some("null"))).await, StatusCode::FORBIDDEN);
        assert_eq!(
            status(ws_req(Some("https://evil.example"))).await,
            StatusCode::FORBIDDEN
        );
        assert_ne!(
            status(ws_req(Some("http://127.0.0.1:47810"))).await,
            StatusCode::FORBIDDEN
        );
        assert_ne!(
            status(ws_req(Some("http://localhost:5173"))).await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn state_changing_request_requires_origin() {
        let req = get_req("/api/health", "127.0.0.1:47810")
            .method(Method::POST)
            .body(Body::empty())
            .unwrap();
        assert_eq!(status(req).await, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_gets_csp_and_spa_fallback() {
        for uri in ["/admin", "/admin/settings"] {
            let res = app()
                .oneshot(get_req(uri, "127.0.0.1:47810").body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "{uri}");
            assert_eq!(res.headers()[header::CONTENT_SECURITY_POLICY], ADMIN_CSP);
        }
        // Same-origin pages could reach the admin IPC capability if framed.
        assert!(ADMIN_CSP.contains("frame-src 'none'"));
    }

    #[tokio::test]
    async fn overlay_gets_overlay_csp_and_assets_get_cors() {
        let res = app()
            .oneshot(
                get_req("/overlay/chat", "127.0.0.1:47810")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.headers()[header::CONTENT_SECURITY_POLICY], OVERLAY_CSP);

        let asset = Web::iter()
            .find(|p| p.starts_with("assets/"))
            .expect("built assets");
        let res = app()
            .oneshot(
                get_req(&format!("/{asset}"), "127.0.0.1:47810")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
    }
}
