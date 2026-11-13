// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{path::PathBuf, sync::Arc};

use host::{BoundHost, HostConfig, HostError, settings};
use integrations::secrets::{CredentialManager, SecretStore};
use secrecy::{ExposeSecret, SecretString};
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder, ipc::CapabilityBuilder};
use tauri_plugin_opener::OpenerExt;

const ADMIN: &str = "admin";
const PORT_PAGE: &str = "port";
const TWITCH_ACTIVATE_URL: &str = "https://www.twitch.tv/activate";

struct AppState {
    admin_token: SecretString,
    config_dir: PathBuf,
    twitch_secrets: Arc<dyn SecretStore>,
}

fn main() {
    tauri::Builder::default()
        // Registered first: a second instance hands off before it ever tries to bind the port.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            focus_main_window(app)
        }))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            CredentialManager::init()?;
            app.manage(AppState {
                admin_token: random_token()?,
                config_dir: app.path().app_config_dir()?,
                twitch_secrets: CredentialManager::new("twitch")?,
            });
            let handle = app.handle().clone();
            let port = settings::load(&handle.state::<AppState>().config_dir).port;
            match tauri::async_runtime::block_on(host::bind(host_config(&handle, port))) {
                Ok(bound) => start(&handle, bound, port)?,
                Err(HostError::PortInUse(_)) => open_port_page(&handle)?,
                Err(e) => return Err(e.into()),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            retry_with_port,
            get_admin_token,
            open_twitch_activate
        ])
        .run(tauri::generate_context!())
        .expect("error while running Streamer Toolkit");
}

/// 32 random bytes as hex; generated per launch and kept only in memory.
fn random_token() -> Result<SecretString, getrandom::Error> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)?;
    Ok(bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        .into())
}

/// Called by the small port-change page after the user enters a new port.
/// Errors are stable codes; the page translates them.
#[tauri::command]
async fn retry_with_port(app: AppHandle, port: u16) -> Result<(), &'static str> {
    if port < 1024 {
        return Err("invalid_port");
    }
    let bound = match host::bind(host_config(&app, port)).await {
        Ok(bound) => bound,
        Err(HostError::PortInUse(_)) => return Err("port_in_use"),
        Err(_) => return Err("bind_failed"),
    };
    settings::update(&app.state::<AppState>().config_dir, |s| s.port = port)
        .map_err(|_| "save_failed")?;
    start(&app, bound, port).map_err(|_| "start_failed")?;
    if let Some(w) = app.get_webview_window(PORT_PAGE) {
        let _ = w.close();
    }
    Ok(())
}

/// The admin window fetches the token once and keeps it in memory (never localStorage).
#[tauri::command]
fn get_admin_token(state: State<'_, AppState>) -> String {
    state.admin_token.expose_secret().to_owned()
}

/// Opens Twitch's fixed activation page in the system browser; the URL is not caller-controlled.
#[tauri::command]
fn open_twitch_activate(app: AppHandle) -> Result<(), &'static str> {
    app.opener()
        .open_url(TWITCH_ACTIVATE_URL, None::<&str>)
        .map_err(|_| "open_failed")
}

fn host_config(app: &AppHandle, port: u16) -> HostConfig {
    let state = app.state::<AppState>();
    // Only apps/desktop sees cfg(dev); the Host receives the extra dev origin as runtime config.
    let extra_origins = if cfg!(dev) {
        app.config()
            .build
            .dev_url
            .as_ref()
            .map(|u| vec![u.origin().ascii_serialization()])
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    HostConfig {
        port,
        extra_origins,
        admin_token: state.admin_token.expose_secret().to_owned().into(),
        config_dir: state.config_dir.clone(),
        twitch_secrets: state.twitch_secrets.clone(),
        twitch_oauth_base: integrations::twitch::auth::OAUTH_BASE.into(),
    }
}

fn start(app: &AppHandle, bound: BoundHost, port: u16) -> tauri::Result<()> {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = bound.serve().await {
            eprintln!("host stopped: {e}");
        }
    });
    open_admin_window(app, port)
}

fn open_admin_window(app: &AppHandle, port: u16) -> tauri::Result<()> {
    let url = if cfg!(dev) {
        // Dev: served by Vite through devUrl (local origin, static capability in capabilities/admin.json).
        WebviewUrl::App("admin/index.html".into())
    } else {
        // Release: the Host URL is a remote origin, so its capability is created for the bound port only.
        // Tauri matches IPC calls by their Origin header, which has no path; on_navigation keeps the window on /admin.
        app.add_capability(
            CapabilityBuilder::new("admin-remote")
                .remote(format!("http://127.0.0.1:{port}"))
                .window(ADMIN)
                .permission("allow-get-admin-token")
                .permission("allow-open-twitch-activate"),
        )?;
        let host_url = format!("http://127.0.0.1:{port}/admin");
        WebviewUrl::External(host_url.parse().expect("valid admin URL"))
    };
    let allowed_origin = format!("http://127.0.0.1:{port}");
    WebviewWindowBuilder::new(app, ADMIN, url)
        .title("Streamer Toolkit")
        .inner_size(1100.0, 720.0)
        // Only /admin pages on the app's own origin (dev: the Vite origin).
        .on_navigation(move |url| {
            let origin = url.origin().ascii_serialization();
            (origin == allowed_origin || cfg!(dev)) && url.path().starts_with("/admin")
        })
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .build()?;
    Ok(())
}

fn open_port_page(app: &AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, PORT_PAGE, WebviewUrl::App("port/index.html".into()))
        .title("Streamer Toolkit")
        .inner_size(480.0, 320.0)
        .resizable(false)
        .build()?;
    Ok(())
}

fn focus_main_window(app: &AppHandle) {
    let window = app
        .get_webview_window(ADMIN)
        .or_else(|| app.get_webview_window(PORT_PAGE));
    if let Some(w) = window {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}
