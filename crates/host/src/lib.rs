//! Local Host: HTTP + WebSocket bound to 127.0.0.1 (see docs/architecture.md §5).

mod admin;
mod backup;
mod chat_dock;
pub mod db;
mod home_tiles;
mod obs;
mod openapi;
mod overlay;
mod queue;
mod tab_bar;
mod templates;
mod text_size;
mod update;

pub use backup::{BackupError, apply_pending};
pub use openapi::openapi;

pub use alerts::Timing as AlertTiming;
pub use checkin::Timing as CheckinTiming;
pub use toolkit_core::presentation::DEFAULT_GAP as PRESENTATION_GAP;
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

// frame-src is added per port by `admin_csp`.
const ADMIN_CSP: &str = "script-src 'self'; connect-src 'self' ipc: http://ipc.localhost; \
style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:";
const OVERLAY_CSP: &str = "script-src 'self'; object-src 'none'";
/// The only pages the admin window may frame: the chat and alert overlays as template previews, which the admin UI
/// puts in `sandbox="allow-scripts"` iframes. The release IPC capability covers the whole Host origin, so no other
/// same-origin page may be framed.
const PREVIEW_PATHS: [&str; 2] = ["/overlay/chat/", "/overlay/alerts/"];

fn admin_csp(hosts: &[String]) -> HeaderValue {
    let frames: Vec<String> = hosts
        .iter()
        .flat_map(|h| PREVIEW_PATHS.map(|p| format!("http://{h}{p}")))
        .collect();
    HeaderValue::from_str(&format!("{ADMIN_CSP}; frame-src {}", frames.join(" ")))
        .expect("hosts are ASCII")
}

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
    /// How long alerts show; the default except in tests.
    pub alert_timing: AlertTiming,
    /// How long stamp cards show; the default except in tests.
    pub checkin_timing: CheckinTiming,
    /// The pause between two alerts or stamp cards; [`PRESENTATION_GAP`] except in tests.
    pub presentation_gap: std::time::Duration,
    /// The database, opened and migrated by [`db::open`] before the Host starts.
    pub db: db::SqlitePool,
    /// The app's version, recorded in backups and compared with the newest release.
    pub app_version: String,
    /// GitHub's latest-release API for the new-version notice; `None` (dev builds, tests): never checked.
    pub latest_release_url: Option<String>,
    /// Restarts the app; a confirmed restore is applied on the next start.
    pub restart: Arc<dyn Fn() + Send + Sync>,
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
    let csp = admin_csp(&guard_state.hosts);
    let tls = integrations::tls::client_config().map_err(std::io::Error::other)?;
    let bus = Arc::new(toolkit_core::bus::EventBus::default());
    let modules = Arc::new(toolkit_core::modules::Modules::load(
        config.data_dir.clone(),
    ));
    let chat = Arc::new(chat::ChatService::new(bus.clone(), modules.clone()));
    let overlays = Arc::new(overlay::OverlayConnections::default());
    let on_air = overlays.clone();
    let presenter =
        toolkit_core::presentation::Presenter::start(config.presentation_gap, move |stage| {
            on_air.count(overlay::OverlayKind::from(stage)) > 0
        });
    queue::follow(&presenter, bus.clone());
    let queue_routes = queue::routes(presenter.clone(), bus.clone());
    let sounds = Arc::new(sound::SoundService::new(
        config.data_dir.clone(),
        modules.clone(),
        bus.clone(),
        presenter.clone(),
    ));
    let alerts = Arc::new(alerts::AlertsService::new(
        bus.clone(),
        modules.clone(),
        config.alert_timing,
        presenter.clone(),
        sounds.clone(),
    ));
    let http = integrations::tls::https_client(tls.clone());
    let twitch = Arc::new(tokio::sync::RwLock::new(
        integrations::twitch::auth::TwitchAuth::new(
            settings::load(&config.data_dir).twitch_client_id,
            &config.twitch.oauth,
            http.clone(),
            config.twitch_secrets.clone(),
        ),
    ));
    let profiles = Arc::new(admin::TwitchProfiles {
        helix: config.twitch.helix.clone(),
        http: http.clone(),
        twitch: twitch.clone(),
    });
    let checkin = Arc::new(checkin::start(
        config.db.clone(),
        bus.clone(),
        modules.clone(),
        config.data_dir.clone(),
        profiles,
        sounds.clone(),
        config.checkin_timing,
        presenter,
    ));
    let outbox = toolkit_core::outbox::Outbox::new(
        config.db.clone(),
        modules.clone(),
        checkin::subscriptions(config.db.clone(), &checkin),
    );
    outbox.start();
    let redemption = Arc::new(redemption::start(
        config.db.clone(),
        bus.clone(),
        modules.clone(),
        outbox.clone(),
    ));
    let stream = Arc::new(stream::start(config.db.clone(), bus.clone()));
    let obs = obs::ObsLink::start(config.data_dir.clone(), config.obs_secrets, bus.clone());
    let templates = templates::Templates::start(&config.data_dir, bus.clone());
    let home_tiles = home_tiles::routes(config.data_dir.clone());
    let chat_dock = chat_dock::routes(config.data_dir.clone());
    let tab_bar = tab_bar::routes(config.data_dir.clone());
    let text_size = text_size::routes(config.data_dir.clone());
    let updates = update::start(
        config.data_dir.clone(),
        config.app_version.clone(),
        http.clone(),
        config.latest_release_url,
    );
    let backup = backup::routes(backup::BackupState {
        db: config.db.clone(),
        data_dir: config.data_dir.clone(),
        app_version: config.app_version.clone(),
        restart: config.restart,
        lock: Default::default(),
    });
    let admin = Arc::new(admin::AdminState::new(
        config.admin_token,
        config.data_dir,
        config.twitch_secrets,
        config.twitch,
        tls,
        http,
        twitch,
        chat,
        alerts,
        redemption.clone(),
        checkin.clone(),
        sounds.clone(),
        stream,
        outbox,
        modules,
    ));
    Ok(Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .merge(
            Router::new()
                .route("/ws/overlay", get(overlay::ws))
                .with_state(overlay::OverlayState {
                    bus,
                    connections: overlays.clone(),
                }),
        )
        .merge(
            overlay::admin_routes(overlays).layer(middleware::from_fn_with_state(
                admin.clone(),
                admin::require_token,
            )),
        )
        .merge(obs::routes(obs).layer(middleware::from_fn_with_state(
            admin.clone(),
            admin::require_token,
        )))
        .merge(
            redemption::routes(config.db.clone(), redemption.clone()).layer(
                middleware::from_fn_with_state(admin.clone(), admin::require_token),
            ),
        )
        .merge(
            checkin::routes(config.db.clone(), checkin).layer(middleware::from_fn_with_state(
                admin.clone(),
                admin::require_token,
            )),
        )
        .merge(
            sound::routes(sounds.clone()).layer(middleware::from_fn_with_state(
                admin.clone(),
                admin::require_token,
            )),
        )
        .merge(sound::file_routes(sounds))
        .merge(home_tiles.layer(middleware::from_fn_with_state(
            admin.clone(),
            admin::require_token,
        )))
        .merge(chat_dock.layer(middleware::from_fn_with_state(
            admin.clone(),
            admin::require_token,
        )))
        .merge(tab_bar.layer(middleware::from_fn_with_state(
            admin.clone(),
            admin::require_token,
        )))
        .merge(text_size.layer(middleware::from_fn_with_state(
            admin.clone(),
            admin::require_token,
        )))
        .merge(queue_routes.layer(middleware::from_fn_with_state(
            admin.clone(),
            admin::require_token,
        )))
        .merge(backup.layer(middleware::from_fn_with_state(
            admin.clone(),
            admin::require_token,
        )))
        .merge(
            update::routes(updates).layer(middleware::from_fn_with_state(
                admin.clone(),
                admin::require_token,
            )),
        )
        .merge(
            templates::admin_routes(templates.clone()).layer(middleware::from_fn_with_state(
                admin.clone(),
                admin::require_token,
            )),
        )
        .merge(
            Router::new()
                .route("/templates/chat/{*path}", get(templates::serve))
                .route(
                    "/templates/built-in/chat/{id}/{*path}",
                    get(templates::serve_built_in),
                )
                .route("/templates/alerts/{*path}", get(templates::serve_alerts))
                .route(
                    "/templates/built-in/alerts/{id}/{*path}",
                    get(templates::serve_built_in_alerts),
                )
                .with_state(templates),
        )
        .merge(admin::routes(admin))
        .fallback(move |uri: Uri| std::future::ready(static_file(&uri, &csp)))
        .layer(middleware::from_fn_with_state(guard_state, guard))
        .layer(middleware::from_fn(templates::headers)))
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

fn static_file(uri: &Uri, admin_csp: &HeaderValue) -> Response {
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
        headers.insert(header::CONTENT_SECURITY_POLICY, admin_csp.clone());
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
            alert_timing: AlertTiming::default(),
            checkin_timing: CheckinTiming::default(),
            presentation_gap: PRESENTATION_GAP,
            db: db::memory(),
            app_version: "0.0.0-test".into(),
            latest_release_url: None,
            restart: Arc::new(|| {}),
        }
    }

    #[tokio::test]
    async fn backup_routes_require_the_token() {
        for (method, uri) in [
            (Method::GET, "/api/admin/backup"),
            (Method::POST, "/api/admin/backup/restore/preview"),
            (Method::POST, "/api/admin/backup/restore"),
        ] {
            let req = get_req(uri, "127.0.0.1:47810")
                .method(method)
                .header(header::ORIGIN, "http://127.0.0.1:47810")
                .body(Body::empty())
                .unwrap();
            assert_eq!(status(req).await, StatusCode::UNAUTHORIZED, "{uri}");
        }
    }

    #[tokio::test]
    async fn update_routes_require_the_token() {
        for (method, uri) in [
            (Method::GET, "/api/admin/update"),
            (Method::PUT, "/api/admin/update/check"),
            (Method::PUT, "/api/admin/update/dismiss"),
        ] {
            let req = get_req(uri, "127.0.0.1:47810")
                .method(method)
                .header(header::ORIGIN, "http://127.0.0.1:47810")
                .body(Body::empty())
                .unwrap();
            assert_eq!(status(req).await, StatusCode::UNAUTHORIZED, "{uri}");
        }
    }

    /// Every documented admin route, so one added later is covered without touching this test.
    #[tokio::test]
    async fn every_admin_route_in_the_openapi_document_requires_the_token() {
        let doc = openapi::openapi();
        let mut checked = Vec::new();
        for (path, item) in doc.paths.paths.iter() {
            if !path.starts_with("/api/admin/") {
                continue;
            }
            let uri = path
                .split('/')
                .map(|s| if s.starts_with('{') { "x" } else { s })
                .collect::<Vec<_>>()
                .join("/");
            for (method, documented) in [
                (Method::GET, item.get.is_some()),
                (Method::PUT, item.put.is_some()),
                (Method::POST, item.post.is_some()),
                (Method::DELETE, item.delete.is_some()),
                (Method::PATCH, item.patch.is_some()),
            ] {
                if !documented {
                    continue;
                }
                let req = get_req(&uri, "127.0.0.1:47810")
                    .method(method.clone())
                    .header(header::ORIGIN, "http://127.0.0.1:47810")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap();
                assert_eq!(
                    status(req).await,
                    StatusCode::UNAUTHORIZED,
                    "{method} {path}"
                );
                checked.push(format!("{method} {path}"));
            }
        }
        // The document really lists the admin API (the twitch, backup, sound and check-in groups at least).
        for route in [
            "GET /api/admin/twitch",
            "POST /api/admin/backup/restore",
            "DELETE /api/admin/sounds/{id}",
            "GET /api/admin/checkin/viewers",
        ] {
            assert!(
                checked.iter().any(|c| c == route),
                "{route} not checked: {checked:?}"
            );
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
    async fn checkin_can_be_closed_and_its_dead_letters_handled() {
        let dir = std::env::temp_dir().join(format!("st-host-close-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut config = test_config();
        config.data_dir = dir.clone();
        config.db = db::open(&dir).await.unwrap();
        let app = router(config).unwrap();
        let call = |method: Method, uri: &str, body: Option<serde_json::Value>| {
            let mut b = get_req(uri, "127.0.0.1:47810")
                .method(method)
                .header(header::ORIGIN, "http://127.0.0.1:47810")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"));
            if body.is_some() {
                b = b.header(header::CONTENT_TYPE, "application/json");
            }
            let req = b
                .body(body.map_or_else(Body::empty, |v| Body::from(v.to_string())))
                .unwrap();
            let app = app.clone();
            async move {
                let res = app.oneshot(req).await.unwrap();
                let status = res.status();
                let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
                    .await
                    .unwrap();
                (
                    status,
                    serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_default(),
                )
            }
        };
        let close = serde_json::json!({ "enabled": false, "close": true });
        let (status, _) = call(Method::PUT, "/api/admin/modules/checkin", Some(close)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, list) = call(Method::GET, "/api/admin/modules", None).await;
        let checkin = list
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == "checkin")
            .unwrap();
        assert_eq!(
            (&checkin["enabled"], &checkin["closed"]),
            (&false.into(), &true.into())
        );

        let enable = serde_json::json!({ "enabled": true });
        let (status, _) = call(Method::PUT, "/api/admin/modules/checkin", Some(enable)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, outbox) = call(Method::GET, "/api/admin/checkin/outbox", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            outbox,
            serde_json::json!({ "pending": 0, "oldest_pending_at": null, "dead_letters": [] })
        );
        for action in ["retry", "skip"] {
            let (status, body) = call(
                Method::POST,
                &format!("/api/admin/checkin/outbox/nope/{action}"),
                None,
            )
            .await;
            assert_eq!(
                (status, &body["code"]),
                (StatusCode::NOT_FOUND, &"outbox.not_found".into())
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn redemption_records_need_the_token() {
        let dir = std::env::temp_dir().join(format!("st-host-redemption-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut config = test_config();
        config.db = db::open(&dir).await.unwrap();
        let app = router(config).unwrap();
        let req = |token: Option<&str>| {
            let b = get_req("/api/admin/redemptions", "127.0.0.1:47810");
            match token {
                Some(t) => b.header(header::AUTHORIZATION, format!("Bearer {t}")),
                None => b,
            }
            .body(Body::empty())
            .unwrap()
        };
        for token in [None, Some("nope")] {
            let res = app.clone().oneshot(req(token)).await.unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        }
        let res = app.clone().oneshot(req(Some(TOKEN))).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            serde_json::json!({ "items": [], "total": 0, "failed_writes": 0 })
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn sound_library_needs_the_token_but_sound_files_do_not() {
        for (token, expected) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some(TOKEN), StatusCode::OK),
        ] {
            let b = get_req("/api/admin/sounds", "127.0.0.1:47810");
            let b = match token {
                Some(t) => b.header(header::AUTHORIZATION, format!("Bearer {t}")),
                None => b,
            };
            assert_eq!(status(b.body(Body::empty()).unwrap()).await, expected);
        }
        let upload = Request::post("/api/admin/sounds?name=a.wav")
            .header(header::HOST, "127.0.0.1:47810")
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .body(Body::from(&b"RIFF\0\0\0\0WAVE"[..]))
            .unwrap();
        assert_eq!(status(upload).await, StatusCode::UNAUTHORIZED);
        // The file route is public (overlays have no token) and answers unknown IDs with 404, not 401.
        let file = get_req(
            "/sounds/0190a5b0-0000-7000-8000-000000000000",
            "127.0.0.1:47810",
        )
        .body(Body::empty())
        .unwrap();
        assert_eq!(status(file).await, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn checkin_settings_need_the_token() {
        for (token, expected) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some("nope"), StatusCode::UNAUTHORIZED),
            (Some(TOKEN), StatusCode::OK),
        ] {
            let b = get_req("/api/admin/checkin/settings", "127.0.0.1:47810");
            let b = match token {
                Some(t) => b.header(header::AUTHORIZATION, format!("Bearer {t}")),
                None => b,
            };
            assert_eq!(status(b.body(Body::empty()).unwrap()).await, expected);
        }
    }

    #[tokio::test]
    async fn simulated_check_ins_are_validated() {
        let app = app();
        for (body, expected) in [
            (r#"{"userName":"Sim","stamps":10}"#, (204, None)),
            (r#"{"stamps":0}"#, (400, Some("simulate.invalid_stamps"))),
            (r#"{"stamps":11}"#, (400, Some("simulate.invalid_stamps"))),
            (
                r#"{"userName":"Sim"}"#,
                (400, Some("simulate.invalid_checkin")),
            ),
        ] {
            let req = get_req("/api/admin/simulate/checkin", "127.0.0.1:47810")
                .method(Method::POST)
                .header(header::ORIGIN, "http://127.0.0.1:47810")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap();
            let res = app.clone().oneshot(req).await.unwrap();
            let status = res.status().as_u16();
            let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap();
            let code = serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|v| v["code"].as_str().map(str::to_owned));
            assert_eq!((status, code.as_deref()), expected, "{body}");
        }
    }

    #[tokio::test]
    async fn checkin_rewards_need_a_sign_in() {
        let req = get_req("/api/admin/checkin/rewards", "127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["code"],
            "twitch.sign_in_required"
        );
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
            alert_timing: AlertTiming::default(),
            checkin_timing: CheckinTiming::default(),
            presentation_gap: PRESENTATION_GAP,
            db: db::memory(),
            app_version: "0.0.0-test".into(),
            latest_release_url: None,
            restart: Arc::new(|| {}),
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
    async fn ask_before_close_is_the_inverse_of_skip_close_hint() {
        let dir = std::env::temp_dir().join(format!("st-host-general-{}", std::process::id()));
        settings::update(&dir, |s| s.skip_close_hint = true).unwrap();
        let mut config = test_config();
        config.data_dir = dir.clone();
        let app = router(config).unwrap();
        let get = || {
            get_req("/api/admin/general", "127.0.0.1:47810")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap()
        };
        let read = |res: axum::response::Response| async {
            let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["ask_before_close"].clone()
        };

        assert_eq!(read(app.clone().oneshot(get()).await.unwrap()).await, false);
        let put = get_req("/api/admin/general", "127.0.0.1:47810")
            .method(Method::PUT)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"ask_before_close":true}"#))
            .unwrap();
        let res = app.clone().oneshot(put).await.unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert!(!settings::load(&dir).skip_close_hint);
        assert_eq!(read(app.oneshot(get()).await.unwrap()).await, true);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn broken_settings_are_reported_and_never_saved_over() {
        let dir = std::env::temp_dir().join(format!("st-host-broken-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let broken = r#"{"skip_close_hint":tru"#;
        std::fs::write(settings::path(&dir), broken).unwrap();
        let mut config = test_config();
        config.data_dir = dir.clone();
        let app = router(config).unwrap();
        let get = get_req("/api/admin/general", "127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();
        let body = axum::body::to_bytes(app.clone().oneshot(get).await.unwrap().into_body(), 4096)
            .await
            .unwrap();
        let general: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(general["settings_unreadable"], true);

        let put = get_req("/api/admin/general", "127.0.0.1:47810")
            .method(Method::PUT)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"ask_before_close":false}"#))
            .unwrap();
        let res = app.oneshot(put).await.unwrap();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            std::fs::read_to_string(settings::path(&dir)).unwrap(),
            broken
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
    async fn simulated_alert_answers_bad_bodies_with_an_error_code() {
        let send = |body: &'static str| {
            get_req("/api/admin/simulate/alert", "127.0.0.1:47810")
                .method(Method::POST)
                .header(header::ORIGIN, "http://127.0.0.1:47810")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap()
        };
        for (body, code) in [
            (r#"{"kind":"nope"}"#, Some("simulate.invalid_alert")),
            (
                r#"{"kind":"raid","amount":"many"}"#,
                Some("simulate.invalid_alert"),
            ),
            (
                r#"{"kind":"raid","amount":0}"#,
                Some("simulate.invalid_amount"),
            ),
            (r#"{"kind":"raid","amount":5}"#, None),
        ] {
            let res = app().oneshot(send(body)).await.unwrap();
            match code {
                Some(code) => {
                    assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{body}");
                    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
                        .await
                        .unwrap();
                    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(json["code"], code, "{body}");
                }
                None => assert_eq!(res.status(), StatusCode::NO_CONTENT, "{body}"),
            }
        }
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

    #[tokio::test]
    async fn home_tiles_persist_and_reject_bad_bodies() {
        let dir = std::env::temp_dir().join(format!("st-host-tiles-{}", std::process::id()));
        let app_for = |dir: &PathBuf| {
            let mut config = test_config();
            config.data_dir = dir.clone();
            router(config).unwrap()
        };
        let put = |body: &str| {
            get_req("/api/admin/home-tiles", "127.0.0.1:47810")
                .method(Method::PUT)
                .header(header::ORIGIN, "http://127.0.0.1:47810")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap()
        };
        let get = |app: Router| async move {
            let res = app
                .oneshot(
                    get_req("/api/admin/home-tiles", "127.0.0.1:47810")
                        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK);
            let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()
        };

        let app = app_for(&dir);
        assert_eq!(get(app.clone()).await, serde_json::Value::Null);
        let res = app.clone().oneshot(put("[]")).await.unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        // A new router on the same data folder stands in for a restart.
        assert_eq!(get(app_for(&dir)).await, serde_json::json!([]));

        let res = app
            .clone()
            .oneshot(put(r#"["chat","chat"]"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert_eq!(get(app.clone()).await, serde_json::json!(["chat"]));

        let res = app.clone().oneshot(put("null")).await.unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert_eq!(get(app.clone()).await, serde_json::Value::Null);
        let saved = std::fs::read_to_string(settings::path(&dir)).unwrap();
        assert!(!saved.contains("home_tiles"), "{saved}");

        let too_many = serde_json::to_string(&vec!["a"; 51]).unwrap();
        for bad in [r#"["Chat"]"#, r#"[1]"#, "{}", "not json", too_many.as_str()] {
            let res = app.clone().oneshot(put(bad)).await.unwrap();
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{bad}");
            let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["code"], "home.invalid_tiles", "{bad}");
        }
        assert_eq!(
            get(app.clone()).await,
            serde_json::Value::Null,
            "bad bodies change nothing"
        );
        let unauthorized = get_req("/api/admin/home-tiles", "127.0.0.1:47810")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.oneshot(unauthorized).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn clearing_and_stopping_the_queue_need_the_token() {
        let post = |path: &str, token: bool| {
            let mut b = get_req(path, "127.0.0.1:47810")
                .method(Method::POST)
                .header(header::ORIGIN, "http://127.0.0.1:47810");
            if token {
                b = b.header(header::AUTHORIZATION, format!("Bearer {TOKEN}"));
            }
            b.body(Body::empty()).unwrap()
        };
        let body = |res: axum::response::Response| async move {
            let bytes = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
        };
        let res = app()
            .oneshot(post("/api/admin/queue/clear", true))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(body(res).await, serde_json::json!({"cancelled": 0}));
        let res = app()
            .oneshot(post("/api/admin/queue/stop", true))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            body(res).await,
            serde_json::json!({"stopped": false, "cancelled": 0})
        );
        for path in ["/api/admin/queue/clear", "/api/admin/queue/stop"] {
            assert_eq!(status(post(path, false)).await, StatusCode::UNAUTHORIZED);
        }
    }

    #[tokio::test]
    async fn overlay_counts_need_the_token() {
        let res = app()
            .oneshot(
                get_req("/api/admin/overlays", "127.0.0.1:47810")
                    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            serde_json::json!({"chat": 0, "alerts": 0, "checkin": 0, "sound": 0, "other": 0})
        );
        let unauthorized = get_req("/api/admin/overlays", "127.0.0.1:47810")
            .body(Body::empty())
            .unwrap();
        assert_eq!(status(unauthorized).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn text_size_persists_and_rejects_bad_bodies() {
        let dir = std::env::temp_dir().join(format!("st-host-textsize-{}", std::process::id()));
        let app_for = |dir: &PathBuf| {
            let mut config = test_config();
            config.data_dir = dir.clone();
            router(config).unwrap()
        };
        let put = |body: &str| {
            get_req("/api/admin/text-size", "127.0.0.1:47810")
                .method(Method::PUT)
                .header(header::ORIGIN, "http://127.0.0.1:47810")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap()
        };
        let get = |app: Router| async move {
            let res = app
                .oneshot(
                    get_req("/api/admin/text-size", "127.0.0.1:47810")
                        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK);
            let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()
        };

        let app = app_for(&dir);
        assert_eq!(get(app.clone()).await, serde_json::Value::Null);
        let res = app.clone().oneshot(put("130")).await.unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        // A new router on the same data folder stands in for a restart.
        assert_eq!(get(app_for(&dir)).await, serde_json::json!(130));
        for bad in ["80", "160", "105", "\"130\"", "not json"] {
            let res = app.clone().oneshot(put(bad)).await.unwrap();
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{bad}");
        }
        assert_eq!(
            get(app.clone()).await,
            serde_json::json!(130),
            "bad bodies change nothing"
        );
        let unauthorized = get_req("/api/admin/text-size", "127.0.0.1:47810")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.oneshot(unauthorized).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn tab_bar_persists_and_rejects_bad_bodies() {
        let dir = std::env::temp_dir().join(format!("st-host-tabbar-{}", std::process::id()));
        let app_for = |dir: &PathBuf| {
            let mut config = test_config();
            config.data_dir = dir.clone();
            router(config).unwrap()
        };
        let put = |body: &str| {
            get_req("/api/admin/tab-bar", "127.0.0.1:47810")
                .method(Method::PUT)
                .header(header::ORIGIN, "http://127.0.0.1:47810")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap()
        };
        let get = |app: Router| async move {
            let res = app
                .oneshot(
                    get_req("/api/admin/tab-bar", "127.0.0.1:47810")
                        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK);
            let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()
        };

        let app = app_for(&dir);
        assert_eq!(get(app.clone()).await, serde_json::Value::Null);
        let res = app
            .clone()
            .oneshot(put(
                r#"{"groups":["media","home","media"],"position":"bottom"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        // A new router on the same data folder stands in for a restart.
        assert_eq!(
            get(app_for(&dir)).await,
            serde_json::json!({ "groups": ["media", "home"], "position": "bottom" })
        );
        let res = app
            .clone()
            .oneshot(put(r#"{"groups":[],"position":"top"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            get(app.clone()).await,
            serde_json::json!({ "groups": [], "position": "top" }),
            "an empty list pins nothing, unlike never edited"
        );

        let too_many = format!(
            r#"{{"groups":{},"position":"top"}}"#,
            serde_json::to_string(&vec!["a"; 21]).unwrap()
        );
        for bad in [
            r#"{"groups":["Home"],"position":"top"}"#,
            r#"{"groups":["home"],"position":"left"}"#,
            r#"{"groups":["home"]}"#,
            "not json",
            too_many.as_str(),
        ] {
            let res = app.clone().oneshot(put(bad)).await.unwrap();
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{bad}");
        }
        let res = app.clone().oneshot(put("null")).await.unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert_eq!(get(app.clone()).await, serde_json::Value::Null);
        let saved = std::fs::read_to_string(settings::path(&dir)).unwrap();
        assert!(!saved.contains("tab_bar"), "{saved}");

        let unauthorized = get_req("/api/admin/tab-bar", "127.0.0.1:47810")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.oneshot(unauthorized).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn chat_dock_persists_and_rejects_bad_bodies() {
        let dir = std::env::temp_dir().join(format!("st-host-dock-{}", std::process::id()));
        let app_for = |dir: &PathBuf| {
            let mut config = test_config();
            config.data_dir = dir.clone();
            router(config).unwrap()
        };
        let put = |body: &str| {
            get_req("/api/admin/chat-dock", "127.0.0.1:47810")
                .method(Method::PUT)
                .header(header::ORIGIN, "http://127.0.0.1:47810")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap()
        };
        let get = |app: Router| async move {
            let res = app
                .oneshot(
                    get_req("/api/admin/chat-dock", "127.0.0.1:47810")
                        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK);
            let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()
        };

        let app = app_for(&dir);
        assert_eq!(get(app.clone()).await, serde_json::Value::Null);
        let res = app
            .clone()
            .oneshot(put(r#"{"open":true,"width":360}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        // A new router on the same data folder stands in for a restart.
        assert_eq!(
            get(app_for(&dir)).await,
            serde_json::json!({ "open": true, "width": 360 })
        );

        for bad in [
            r#"{"open":true,"width":279}"#,
            r#"{"open":true,"width":561}"#,
            r#"{"open":true}"#,
            "not json",
        ] {
            let res = app.clone().oneshot(put(bad)).await.unwrap();
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{bad}");
        }
        assert_eq!(
            get(app.clone()).await,
            serde_json::json!({ "open": true, "width": 360 }),
            "bad bodies change nothing"
        );
        let unauthorized = get_req("/api/admin/chat-dock", "127.0.0.1:47810")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.oneshot(unauthorized).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
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
            // Same-origin pages could reach the admin IPC capability if framed: only the preview page may be.
            assert_eq!(
                res.headers()[header::CONTENT_SECURITY_POLICY],
                format!(
                    "{ADMIN_CSP}; frame-src http://127.0.0.1:47810/overlay/chat/ \
                     http://127.0.0.1:47810/overlay/alerts/ http://localhost:47810/overlay/chat/ \
                     http://localhost:47810/overlay/alerts/"
                )
            );
        }
        assert!(!ADMIN_CSP.contains("frame-src"));
    }

    #[tokio::test]
    async fn overlay_gets_overlay_csp_and_assets_get_cors() {
        for page in [
            "/overlay/chat",
            "/overlay/alerts",
            "/overlay/checkin",
            "/overlay/sound",
        ] {
            let res = app()
                .oneshot(
                    get_req(page, "127.0.0.1:47810")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "{page}");
            assert_eq!(res.headers()[header::CONTENT_SECURITY_POLICY], OVERLAY_CSP);
        }

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
        (template_router(&dir), dir)
    }

    fn template_router(dir: &std::path::Path) -> Router {
        let mut config = test_config();
        config.data_dir = dir.to_path_buf();
        router(config).unwrap()
    }

    /// `custom` selected, with a copy of the default template in the data folder.
    fn custom_template_app(name: &str) -> (Router, PathBuf) {
        let dir = std::env::temp_dir().join(format!("st-tpl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        settings::update(&dir, |s| s.chat_template = "custom".into()).unwrap();
        templates::tests::write_default(&dir.join("templates/chat"));
        (template_router(&dir), dir)
    }

    #[tokio::test]
    async fn alert_template_is_served_like_the_chat_ones() {
        let app = app();
        let (status, headers, html) = get_body(&app, "/templates/alerts/template.html").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_SECURITY_POLICY], OVERLAY_CSP);
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        // The markers apps/web/src/overlay/alerts/template.ts binds to.
        assert!(html.contains("data-alert-stage") && html.contains("<template data-alert>"));
        let (status, headers, _) = get_body(&app, "/templates/alerts/style.css").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "text/css; charset=utf-8");
        for (uri, expected) in [
            ("/templates/alerts/evil.js", StatusCode::NOT_FOUND),
            (
                "/templates/alerts/../chat/template.html",
                StatusCode::BAD_REQUEST,
            ),
            ("/templates/alerts/%2E%2E/x.css", StatusCode::BAD_REQUEST),
        ] {
            assert_eq!(get_body(&app, uri).await.0, expected, "{uri}");
        }
    }

    fn put_template(id: &str) -> Request {
        get_req("/api/admin/templates/chat", "127.0.0.1:47810")
            .method(Method::PUT)
            .header(header::ORIGIN, "http://127.0.0.1:47810")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(format!(r#"{{"selected":"{id}"}}"#)))
            .unwrap()
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

    async fn template_status(app: &Router) -> serde_json::Value {
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
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
    }

    async fn template_error(app: &Router) -> serde_json::Value {
        template_status(app).await["error"].clone()
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
    async fn first_run_serves_the_built_in_default_with_overlay_headers() {
        let (app, dir) = template_app("default");
        assert!(
            !dir.join("templates/chat").exists(),
            "the user folder is not created"
        );
        assert_eq!(
            template_status(&app).await,
            serde_json::json!({ "selected": "default", "error": null })
        );
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
        // Error answers carry the same headers, also the ones produced before the handler runs.
        for uri in [
            "/templates/chat/nope.css",
            "/templates/chat/%ff.css",
            "/templates/chat/",
            "/templates",
            "/templates/built-in/chat/nope/template.html",
        ] {
            let (status, headers, _) = get_body(&app, uri).await;
            assert!(!status.is_success(), "{uri}");
            assert_eq!(
                headers[header::CONTENT_SECURITY_POLICY],
                OVERLAY_CSP,
                "{uri}"
            );
            assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff", "{uri}");
            assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*", "{uri}");
        }
        let foreign_host = app
            .clone()
            .oneshot(
                get_req("/templates/chat/style.css", "evil.example:47810")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(foreign_host.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            foreign_host.headers()[header::CONTENT_SECURITY_POLICY],
            OVERLAY_CSP
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn switching_templates_applies_and_survives_a_restart() {
        let (app, dir) = template_app("switch");
        let mut no_token = put_template("bubble");
        no_token.headers_mut().remove(header::AUTHORIZATION);
        assert_eq!(
            app.clone().oneshot(no_token).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            app.clone()
                .oneshot(put_template("bubble"))
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(template_status(&app).await["selected"], "bubble");
        let (_, _, body) = get_body(&app, "/templates/chat/template.html").await;
        assert!(body.contains(r#"class="chat bubble""#), "{body}");
        for bad in ["nope", "Bubble", ""] {
            let res = app.clone().oneshot(put_template(bad)).await.unwrap();
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{bad}");
            let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["code"],
                "template.unknown"
            );
        }
        assert_eq!(settings::load(&dir).chat_template, "bubble");
        let restarted = template_router(&dir);
        assert_eq!(template_status(&restarted).await["selected"], "bubble");
        // A hand-edited unknown value means the default.
        settings::update(&dir, |s| s.chat_template = "gone".into()).unwrap();
        assert_eq!(
            template_status(&template_router(&dir)).await["selected"],
            "default"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn every_built_in_template_is_served_for_previews() {
        let (app, dir) = template_app("built-in");
        for id in ["default", "compact", "bubble"] {
            let (status, headers, body) = get_body(
                &app,
                &format!("/templates/built-in/chat/{id}/template.html"),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{id}");
            assert!(body.contains("data-chat-list"), "{id}");
            assert_eq!(headers[header::CONTENT_SECURITY_POLICY], OVERLAY_CSP);
            assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
            let (status, _, _) =
                get_body(&app, &format!("/templates/built-in/chat/{id}/style.css")).await;
            assert_eq!(status, StatusCode::OK, "{id}");
        }
        for uri in [
            "/templates/built-in/chat/custom/template.html",
            "/templates/built-in/chat/default/evil.js",
            "/templates/built-in/chat/default/../bubble/style.css",
            "/templates/built-in/chat/default/..%2fbubble%2fstyle.css",
        ] {
            let (status, _, _) = get_body(&app, uri).await;
            assert!(!status.is_success(), "{uri} -> {status}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn custom_template_serves_assets_in_subfolders() {
        let (app, dir) = custom_template_app("assets");
        assert_eq!(
            template_status(&app).await,
            serde_json::json!({ "selected": "custom", "error": null })
        );
        std::fs::create_dir_all(dir.join("templates/chat/img")).unwrap();
        std::fs::write(dir.join("templates/chat/img/logo.svg"), "<svg/>").unwrap();
        let (status, headers, body) = get_body(&app, "/templates/chat/img/logo.svg").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "image/svg+xml");
        assert_eq!(body, "<svg/>");
        // The user folder is not used while a built-in template is selected.
        app.clone().oneshot(put_template("compact")).await.unwrap();
        let (status, _, _) = get_body(&app, "/templates/chat/img/logo.svg").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn template_route_never_serves_scripts() {
        let (app, dir) = custom_template_app("scripts");
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
        let (app, dir) = custom_template_app("escape");
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
        let (app, dir) = custom_template_app("fallback");
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
    async fn custom_template_change_is_pushed_to_overlays() {
        let dir = std::env::temp_dir().join(format!("st-tpl-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        settings::update(&dir, |s| s.chat_template = "custom".into()).unwrap();
        templates::tests::write_default(&dir.join("templates/chat"));
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

    #[tokio::test]
    async fn user_folder_changes_are_ignored_while_a_built_in_template_is_selected() {
        let dir = std::env::temp_dir().join(format!("st-tpl-quiet-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        templates::tests::write_default(&dir.join("templates/chat"));
        let bus = Arc::new(toolkit_core::bus::EventBus::default());
        let _templates = templates::Templates::start(&dir, bus.clone());
        let (_, mut events) = bus.subscribe();
        std::fs::write(dir.join("templates/chat/style.css"), ".chat{color:red}").unwrap();
        // Two and a half polls.
        let event =
            tokio::time::timeout(std::time::Duration::from_millis(2500), events.recv()).await;
        assert!(event.is_err(), "no reload for an unused folder: {event:?}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
