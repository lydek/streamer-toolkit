//! Local Host: HTTP + WebSocket bound to 127.0.0.1 (see docs/architecture.md §5).

mod admin;
mod backup;
mod chat_display;
mod chat_dock;
mod chat_log;
pub mod db;
mod event_feed;
mod home_tiles;
mod obs;
mod openapi;
mod overlay;
mod overlay_test;
mod queue;
mod stream_summary;
mod tab_bar;
mod templates;
mod text_size;
mod update;

pub use backup::{BackupError, apply_pending};
pub use openapi::openapi;

pub use alerts::Timing as AlertTiming;
pub use chat::ChatService;
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
style-src 'self' 'unsafe-inline'; img-src 'self' data: https://static-cdn.jtvnw.net; font-src 'self' data:";
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
    /// Anonymous chat over IRC, read only for Twitch's first-chat mark.
    pub irc: String,
}

impl Default for TwitchEndpoints {
    fn default() -> Self {
        use integrations::twitch::{auth, eventsub, helix, irc};
        Self {
            oauth: auth::OAUTH_BASE.into(),
            helix: helix::HELIX_BASE.into(),
            eventsub: eventsub::EVENTSUB_URL.into(),
            irc: irc::IRC_URL.into(),
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
    chat: Arc<ChatService>,
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
    let (router, chat) = router_with_chat(config)?;
    Ok(BoundHost {
        listener,
        router,
        port,
        chat,
    })
}

impl BoundHost {
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn chat(&self) -> Arc<ChatService> {
        self.chat.clone()
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
    router_with_chat(config).map(|(router, _)| router)
}

fn router_with_chat(config: HostConfig) -> Result<(Router, Arc<ChatService>), HostError> {
    let guard_state = Guard::new(&config);
    let csp = admin_csp(&guard_state.hosts);
    let tls = integrations::tls::client_config().map_err(std::io::Error::other)?;
    let bus = Arc::new(toolkit_core::bus::EventBus::default());
    let modules = Arc::new(toolkit_core::modules::Modules::load(
        config.data_dir.clone(),
    ));
    let stream = Arc::new(stream::start(config.db.clone(), bus.clone()));
    let chat_sessions = stream.clone();
    let chat = Arc::new(chat::start(
        bus.clone(),
        modules.clone(),
        config.db.clone(),
        move || chat_sessions.session_after_pending(),
    ));
    let overlays = Arc::new(overlay::OverlayConnections::default());
    let on_air = overlays.clone();
    let presenter =
        toolkit_core::presentation::Presenter::start(config.presentation_gap, move |stage| {
            on_air.count(overlay::OverlayKind::from(stage)) > 0
        });
    // Waiting items say whether their overlay is open; follow OBS opening and closing them.
    let (mut changes, refreshed) = (overlays.changes(), presenter.clone());
    tokio::spawn(async move {
        while changes.changed().await.is_ok() {
            refreshed.refresh();
        }
    });
    queue::follow(&presenter, bus.clone());
    let queue_routes = queue::routes(presenter.clone(), bus.clone());
    let overlay_test_routes = overlay_test::routes(bus.clone(), modules.clone());
    let sounds = Arc::new(sound::SoundService::new(
        config.data_dir.clone(),
        modules.clone(),
        bus.clone(),
        presenter.clone(),
    ));
    let music = Arc::new(bgm::BgmService::new(config.data_dir.clone()));
    let music_on_air = overlays.clone();
    let player = bgm::start(
        music.clone(),
        modules.clone(),
        bus.clone(),
        move || music_on_air.count(overlay::OverlayKind::Bgm) > 0,
        overlays.changes(),
    );
    let alerts = Arc::new(alerts::AlertsService::new(
        bus.clone(),
        modules.clone(),
        config.alert_timing,
        presenter.clone(),
        sounds.clone(),
    ));
    let alert_routes = alerts::routes(alerts.clone(), sounds.clone());
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
    let viewers = Arc::new(viewers::start(config.db.clone(), profiles.clone()));
    let chat_display = chat_display::routes(
        config.data_dir.clone(),
        config.db.clone(),
        config.twitch.helix.clone(),
        http.clone(),
        twitch.clone(),
    );
    // Every module's chat commands in one table; a name belongs to one of them.
    let commands = Arc::new(toolkit_core::commands::CommandTable::load(
        config.data_dir.clone(),
        checkin::COMMANDS.to_vec(),
    ));
    let checkin = Arc::new(checkin::start(
        config.db.clone(),
        bus.clone(),
        modules.clone(),
        config.data_dir.clone(),
        profiles,
        sounds.clone(),
        config.checkin_timing,
        presenter,
        commands.clone(),
    ));
    let counter_sessions = stream.clone();
    let counters = counter::start(
        config.db.clone(),
        bus.clone(),
        modules.clone(),
        commands,
        move || counter_sessions.session_after_pending(),
    );
    // Per-stream counters go back to 0 once a stream is live (each status repeats it; the counters keep it to once),
    // and the marathon clock runs only while live.
    let (_, mut stream_events) = bus.subscribe();
    let live_counters = counters.clone();
    tokio::spawn(async move {
        loop {
            match stream_events.recv().await {
                Ok(text) => {
                    let Ok(e) = serde_json::from_str::<serde_json::Value>(&text) else {
                        continue;
                    };
                    if e["type"] != "stream.status" {
                        continue;
                    }
                    let state = match e["payload"]["state"].as_str() {
                        Some("live") => e["payload"]["startedAt"]
                            .as_str()
                            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                            .map(|t| counter::StreamState::Live {
                                started_at: t.to_utc(),
                            }),
                        Some("offline") => Some(counter::StreamState::Offline),
                        _ => None,
                    };
                    if let Some(state) = state {
                        live_counters.stream_status(state).await;
                    }
                }
                // A missed status is repeated by the next check.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });
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
    let sessions = stream.clone();
    // Chat records share the activity module's record retention.
    let records = || -> activity::OtherRecords { vec![Arc::new(chat::ChatRecords)] };
    let activity = Arc::new(activity::start(
        config.db.clone(),
        config.data_dir.clone(),
        move || sessions.session_after_pending(),
        records(),
    ));
    let activity_routes = activity::routes(config.db.clone(), config.data_dir.clone(), records());
    let chat_routes = chat::routes(config.db.clone()).merge(chat_log::routes(config.db.clone()));
    let launched_at = chrono::Utc::now();
    let stream_summary = stream_summary::routes(config.db.clone(), launched_at);
    let feed = event_feed::routes(event_feed::FeedState {
        pool: config.db.clone(),
        launched_at,
        alerts: alerts.clone(),
        sounds: sounds.clone(),
        checkin: checkin.clone(),
    });
    let obs = obs::ObsLink::start(config.data_dir.clone(), config.obs_secrets, bus.clone());
    // Background music follows the scene on air: applied when OBS connects, stopped when it goes away.
    let (mut states, music_player) = (obs.states(), player.clone());
    tokio::spawn(async move {
        loop {
            let scene = states.borrow_and_update().scene.clone();
            music_player.scene_changed(scene);
            if states.changed().await.is_err() {
                return;
            }
        }
    });
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
        chat.clone(),
        alerts,
        redemption.clone(),
        checkin.clone(),
        counters.clone(),
        sounds.clone(),
        stream,
        activity,
        viewers,
        outbox,
        modules,
    ));
    let router =
        Router::new()
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
            .merge(checkin::routes(config.db.clone(), checkin).layer(
                middleware::from_fn_with_state(admin.clone(), admin::require_token),
            ))
            .merge(
                sound::routes(sounds.clone()).layer(middleware::from_fn_with_state(
                    admin.clone(),
                    admin::require_token,
                )),
            )
            .merge(sound::file_routes(sounds))
            .merge(
                bgm::routes(music.clone(), player).layer(middleware::from_fn_with_state(
                    admin.clone(),
                    admin::require_token,
                )),
            )
            .merge(bgm::file_routes(music))
            .merge(
                counter::routes(counters).layer(middleware::from_fn_with_state(
                    admin.clone(),
                    admin::require_token,
                )),
            )
            .merge(overlay_test_routes.layer(middleware::from_fn_with_state(
                admin.clone(),
                admin::require_token,
            )))
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
            .merge(activity_routes.layer(middleware::from_fn_with_state(
                admin.clone(),
                admin::require_token,
            )))
            .merge(stream_summary.layer(middleware::from_fn_with_state(
                admin.clone(),
                admin::require_token,
            )))
            .merge(feed.layer(middleware::from_fn_with_state(
                admin.clone(),
                admin::require_token,
            )))
            .merge(alert_routes.layer(middleware::from_fn_with_state(
                admin.clone(),
                admin::require_token,
            )))
            .merge(chat_routes.layer(middleware::from_fn_with_state(
                admin.clone(),
                admin::require_token,
            )))
            .merge(chat_display.layer(middleware::from_fn_with_state(
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
            .merge(templates::admin_routes(templates.clone()).layer(
                middleware::from_fn_with_state(admin.clone(), admin::require_token),
            ))
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
            .layer(middleware::from_fn(templates::headers));
    Ok((router, chat))
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
mod tests;
