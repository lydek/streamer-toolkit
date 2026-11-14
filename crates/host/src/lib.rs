//! Local Host: HTTP + WebSocket bound to 127.0.0.1 (see docs/architecture.md §5).

mod admin;
mod obs;
mod openapi;
mod overlay;
mod templates;

pub use openapi::openapi;

pub use toolkit_core::{DEFAULT_PORT, settings};

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
    /// The data folder this copy owns (`settings.json`, logs, later the database).
    pub data_dir: PathBuf,
    /// Where the broadcaster's Twitch tokens live (Windows Credential Manager in production).
    pub twitch_secrets: Arc<dyn SecretStore>,
    pub twitch: TwitchEndpoints,
    /// Where the obs-websocket password lives (Windows Credential Manager in production).
    pub obs_secrets: Arc<dyn SecretStore>,
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
    port: u16,
}

/// Binds before serving so the caller can open the port-change page when the port is taken.
/// Port 0 picks a free port; the Host and Origin allowlists use the port actually bound.
pub async fn bind(mut config: HostConfig) -> Result<BoundHost, HostError> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, config.port));
    let listener = TcpListener::bind(addr).await.map_err(|e| match e.kind() {
        std::io::ErrorKind::AddrInUse => HostError::PortInUse(config.port),
        _ => HostError::Io(e),
    })?;
    config.port = listener.local_addr()?.port();
    let port = config.port;
    Ok(BoundHost {
        listener,
        router: router(config)?,
        port,
    })
}

impl BoundHost {
    pub fn port(&self) -> u16 {
        self.port
    }

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
    let bus = Arc::new(toolkit_core::bus::EventBus::default());
    let modules = Arc::new(toolkit_core::modules::Modules::load(
        config.data_dir.clone(),
    ));
    let chat = Arc::new(chat::ChatService::new(bus.clone(), modules.clone()));
    let obs = obs::ObsLink::start(config.data_dir.clone(), config.obs_secrets, bus.clone());
    let templates = templates::Templates::start(&config.data_dir, bus.clone());
    let admin = Arc::new(admin::AdminState::new(
        config.admin_token,
        config.data_dir,
        config.twitch_secrets,
        config.twitch,
        tls,
        chat,
        modules,
    ));
    Ok(Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .merge(
            Router::new()
                .route("/ws/overlay", get(overlay::ws))
                .with_state(bus),
        )
        .merge(obs::routes(obs).layer(middleware::from_fn_with_state(
            admin.clone(),
            admin::require_token,
        )))
        .merge(
            templates::admin_routes(templates.clone()).layer(middleware::from_fn_with_state(
                admin.clone(),
                admin::require_token,
            )),
        )
        .merge(
            Router::new()
                .route("/templates/chat/{*path}", get(templates::serve))
                .with_state(templates),
        )
        .merge(admin::routes(admin))
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
        router(test_config()).unwrap()
    }

    fn test_config() -> HostConfig {
        HostConfig {
            port: 47810,
            extra_origins: vec!["http://localhost:5173".into()],
            admin_token: TOKEN.into(),
            data_dir: std::env::temp_dir().join(format!("st-host-test-{}", std::process::id())),
            twitch_secrets: Arc::new(NoSecrets),
            twitch: TwitchEndpoints {
                oauth: "http://127.0.0.1:1".into(),
                helix: "http://127.0.0.1:1".into(),
                eventsub: "ws://127.0.0.1:1".into(),
            },
            obs_secrets: Arc::new(NoSecrets),
        }
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
            data_dir: dir.clone(),
            twitch_secrets: secrets.clone(),
            twitch: TwitchEndpoints {
                oauth: "http://127.0.0.1:1".into(),
                helix: "http://127.0.0.1:1".into(),
                eventsub: "ws://127.0.0.1:1".into(),
            },
            obs_secrets: Arc::new(NoSecrets),
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

    #[tokio::test]
    async fn obs_password_goes_to_the_secret_store_only() {
        let dir = std::env::temp_dir().join(format!("st-host-obs-{}", std::process::id()));
        let secrets = Arc::new(MemSecrets::default());
        let mut config = test_config();
        config.data_dir = dir.clone();
        config.obs_secrets = secrets.clone();
        let app = router(config).unwrap();
        let put = |body: &'static str| {
            get_req("/api/admin/obs", "127.0.0.1:47810")
                .method(Method::PUT)
                .header(header::ORIGIN, "http://127.0.0.1:47810")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap()
        };

        let res = app
            .clone()
            .oneshot(put(
                r#"{"host":"obs-pc.local","port":4456,"password":"hunter2"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert_eq!(secrets.0.lock().unwrap().as_deref(), Some("hunter2"));
        let saved = std::fs::read_to_string(settings::path(&dir)).unwrap();
        assert!(saved.contains("obs-pc.local"));
        assert!(!saved.contains("hunter2"), "{saved}");

        let res = app
            .clone()
            .oneshot(
                get_req("/api/admin/obs", "127.0.0.1:47810")
                    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let overview: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(overview["port"], 4456);
        assert_eq!(overview["password_set"], true);
        assert!(!String::from_utf8_lossy(&body).contains("hunter2"));

        // Without `password` the stored one stays; an empty one removes it.
        let res = app
            .clone()
            .oneshot(put(r#"{"host":"127.0.0.1","port":4455}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert!(secrets.0.lock().unwrap().is_some());
        let res = app
            .clone()
            .oneshot(put(r#"{"host":"127.0.0.1","port":4455,"password":""}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert!(secrets.0.lock().unwrap().is_none());

        let res = app
            .clone()
            .oneshot(put(r#"{"host":"ws://evil","port":4455}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let unauthorized = get_req("/api/admin/obs", "127.0.0.1:47810")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.oneshot(unauthorized).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn port_zero_binds_a_free_port_and_allows_it() {
        let mut config = test_config();
        config.port = 0;
        let bound = bind(config).await.unwrap();
        let port = bound.port();
        assert_ne!(port, 0);
        let res = bound
            .router
            .oneshot(
                get_req("/api/health", &format!("127.0.0.1:{port}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::OK,
            "the Host allowlist uses the bound port"
        );
    }

    #[tokio::test]
    async fn simulated_chat_validates_text() {
        let send = |body: &'static str| {
            get_req("/api/admin/simulate/chat", "127.0.0.1:47810")
                .method(Method::POST)
                .header(header::ORIGIN, "http://127.0.0.1:47810")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap()
        };
        assert_eq!(
            status(send(r#"{"text":"  "}"#)).await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            status(send(r#"{"text":"hi","userName":"Tester"}"#)).await,
            StatusCode::NO_CONTENT
        );
    }

    #[tokio::test]
    async fn module_switch_rejects_unknown_modules() {
        let req = get_req("/api/admin/modules/nope", "127.0.0.1:47810")
            .method(Method::PUT)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"enabled":false}"#))
            .unwrap();
        assert_eq!(status(req).await, StatusCode::NOT_FOUND);
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

    /// A Host with its own data folder, so template tests can change files without racing each other.
    fn template_app(name: &str) -> (Router, PathBuf) {
        let dir = std::env::temp_dir().join(format!("st-tpl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut config = test_config();
        config.data_dir = dir.clone();
        (router(config).unwrap(), dir)
    }

    async fn get_body(app: &Router, uri: &str) -> (StatusCode, axum::http::HeaderMap, String) {
        let res = app
            .clone()
            .oneshot(get_req(uri, "127.0.0.1:47810").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let (parts, body) = res.into_parts();
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        (
            parts.status,
            parts.headers,
            String::from_utf8_lossy(&bytes).into(),
        )
    }

    async fn template_error(app: &Router) -> serde_json::Value {
        let res = app
            .clone()
            .oneshot(
                get_req("/api/admin/templates/chat", "127.0.0.1:47810")
                    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["error"].clone()
    }

    /// The watcher polls every second; give it a few rounds.
    async fn wait_for_error(app: &Router, code: &str) {
        for _ in 0..50 {
            if template_error(app).await == code {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        panic!("template error never became {code}");
    }

    #[tokio::test]
    async fn first_run_copies_the_default_template_and_serves_it_with_overlay_headers() {
        let (app, dir) = template_app("default");
        assert!(dir.join("templates/chat/template.html").is_file());
        let (status, headers, body) = get_body(&app, "/templates/chat/template.html").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("data-chat-list"));
        assert_eq!(headers[header::CONTENT_TYPE], "text/html; charset=utf-8");
        assert_eq!(headers[header::CONTENT_SECURITY_POLICY], OVERLAY_CSP);
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        assert_eq!(template_error(&app).await, serde_json::Value::Null);
        let no_token = get_req("/api/admin/templates/chat", "127.0.0.1:47810")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.clone().oneshot(no_token).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        // Error answers carry the same headers.
        let (status, headers, _) = get_body(&app, "/templates/chat/nope.css").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(headers[header::CONTENT_SECURITY_POLICY], OVERLAY_CSP);
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn template_route_never_serves_scripts() {
        let (app, dir) = template_app("scripts");
        let chat = dir.join("templates/chat");
        for name in ["evil.js", "evil.mjs", "evil.JS", "data.json"] {
            std::fs::write(chat.join(name), "alert(1)").unwrap();
            let (status, _, body) = get_body(&app, &format!("/templates/chat/{name}")).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{name}");
            assert!(!body.contains("alert"), "{name}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn template_route_rejects_path_escapes() {
        let (app, dir) = template_app("escape");
        std::fs::write(dir.join("secret.css"), "secret").unwrap();
        std::fs::write(dir.join("templates/secret.css"), "secret").unwrap();
        for uri in [
            "/templates/chat/../secret.css",
            "/templates/chat/../../secret.css",
            "/templates/chat/%2e%2e/%2e%2e/secret.css",
            "/templates/chat/..%2f..%2fsecret.css",
            "/templates/chat/..%5c..%5csecret.css",
            "/templates/chat/sub/..%5c..%5c..%5csecret.css",
            "/templates/chat/C:%5cWindows%5cwin.ini.css",
            "/templates/chat//secret.css",
        ] {
            let (status, _, body) = get_body(&app, uri).await;
            assert!(!status.is_success(), "{uri} -> {status}");
            assert!(!body.contains("secret"), "{uri}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn missing_or_broken_template_falls_back_to_the_default() {
        let (app, dir) = template_app("fallback");
        let chat = dir.join("templates/chat");
        std::fs::write(chat.join("template.html"), "<p>broken</p>").unwrap();
        std::fs::write(chat.join("style.css"), "user-css").unwrap();
        wait_for_error(&app, "template.invalid").await;
        let (status, _, body) = get_body(&app, "/templates/chat/template.html").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("data-chat-list"), "default HTML, never blank");
        let (_, _, css) = get_body(&app, "/templates/chat/style.css").await;
        assert!(!css.contains("user-css"), "the default comes as a whole");

        std::fs::remove_file(chat.join("template.html")).unwrap();
        wait_for_error(&app, "template.missing").await;
        let (status, _, body) = get_body(&app, "/templates/chat/template.html").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("data-chat-list"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn template_change_is_pushed_to_overlays() {
        let dir = std::env::temp_dir().join(format!("st-tpl-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bus = Arc::new(toolkit_core::bus::EventBus::default());
        let _templates = templates::Templates::start(&dir, bus.clone());
        let (_, mut events) = bus.subscribe();
        std::fs::write(dir.join("templates/chat/style.css"), ".chat{color:red}").unwrap();
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
            .await
            .expect("notified within a couple of seconds")
            .unwrap();
        let event: serde_json::Value = serde_json::from_str(&event).unwrap();
        assert_eq!(event["type"], "template.changed");
        assert_eq!(event["source"], "system");
        assert_eq!(event["payload"]["overlay"], "chat");
        let _ = std::fs::remove_dir_all(dir);
    }
}
