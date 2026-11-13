// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod data_folder;
mod text;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use host::{BoundHost, HostConfig, HostError, TwitchEndpoints, settings};
use integrations::secrets::{CredentialManager, SecretStore};
use secrecy::{ExposeSecret, SecretString};
use tauri::{
    AppHandle, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder, WindowEvent,
    ipc::CapabilityBuilder,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_opener::OpenerExt;
use tracing_appender::{non_blocking::WorkerGuard, rolling};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

const ADMIN: &str = "admin";
const PORT_PAGE: &str = "port";
const TWITCH_ACTIVATE_URL: &str = "https://www.twitch.tv/activate";
const WEBVIEW2_DOWNLOAD_URL: &str = "https://developer.microsoft.com/microsoft-edge/webview2/";
const LOG_DAYS: usize = 14;

struct AppState {
    admin_token: SecretString,
    data_dir: PathBuf,
    twitch_secrets: Arc<dyn SecretStore>,
    /// Set once the Host listens; lets the tray open the admin window later (e.g. after WebView2 is installed).
    host_port: Mutex<Option<u16>>,
}

fn main() {
    let mut context = tauri::generate_context!();
    let base_identifier = context.config().identifier.clone();
    let data_dir = data_folder::resolve()
        .unwrap_or_else(|e| fail(&text::data_folder_unusable("", &e.to_string())));
    if let Err(e) = data_folder::ensure_writable(&data_dir) {
        fail(&text::data_folder_unusable(
            &data_dir.display().to_string(),
            &e.to_string(),
        ));
    }
    let mut log_guard = init_logs(&data_dir.join("logs"));
    let instance = match data_folder::instance_id(&data_dir) {
        Ok(id) => id,
        Err(e) => fail(&text::data_folder_unusable(
            &data_dir.display().to_string(),
            &e.to_string(),
        )),
    };
    tracing::info!(data_dir = %data_dir.display(), "starting");
    // The single-instance plugin names its lock after the identifier: one running copy per data folder.
    context.config_mut().identifier = format!("{base_identifier}.i{instance}");

    let app = tauri::Builder::default()
        // Registered first: a second copy on the same folder hands off before it ever tries to bind the port.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app)
        }))
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            CredentialManager::init()?;
            app.manage(AppState {
                admin_token: random_token()?,
                data_dir: data_dir.clone(),
                twitch_secrets: CredentialManager::new(&format!("{instance}/twitch"))?,
                host_port: Mutex::new(None),
            });
            create_tray(app.handle())?;
            let handle = app.handle().clone();
            // A folder without settings has no OBS URL yet, so a taken default port can be swapped for a free one.
            let first_run = !settings::path(&data_dir).exists();
            let port = settings::load(&data_dir).port;
            let bound = match tauri::async_runtime::block_on(host::bind(host_config(&handle, port))) {
                Err(HostError::PortInUse(_)) if first_run => {
                    tauri::async_runtime::block_on(host::bind(host_config(&handle, 0)))
                }
                other => other,
            };
            match bound {
                Ok(bound) => {
                    if bound.port() != port || first_run {
                        let chosen = bound.port();
                        if let Err(e) = settings::update(&data_dir, |s| s.port = chosen) {
                            tracing::error!(code = "settings.save_failed", error = %e, "port not saved");
                        }
                    }
                    start(&handle, bound)?
                }
                Err(HostError::PortInUse(port)) => {
                    tracing::warn!(code = "port.in_use", port, "asking for another port");
                    open_port_page(&handle)?
                }
                Err(e) => return Err(e.into()),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            retry_with_port,
            get_admin_token,
            open_twitch_activate
        ])
        .build(context)
        .unwrap_or_else(|e| fail(&e.to_string()));
    app.run(move |app, event| match event {
        // Closing the admin window only hides it; overlays keep running until Quit in the tray.
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::CloseRequested { api, .. },
            ..
        } if label == ADMIN => {
            api.prevent_close();
            if let Some(w) = app.get_webview_window(ADMIN) {
                let _ = w.hide();
            }
        }
        // Keep running in the tray when the port page is closed.
        RunEvent::ExitRequested {
            code: None, api, ..
        } => api.prevent_exit(),
        // Flush buffered log lines before the process ends.
        RunEvent::Exit => drop(log_guard.take()),
        _ => {}
    });
}

/// JSON lines, one file per day, the last `LOG_DAYS` kept. Dev builds also print to stderr.
fn init_logs(dir: &Path) -> Option<WorkerGuard> {
    let appender = rolling::Builder::new()
        .rotation(rolling::Rotation::DAILY)
        .filename_prefix("streamer-toolkit")
        .filename_suffix("log")
        .max_log_files(LOG_DAYS)
        .build(dir)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let console = cfg!(dev).then(|| fmt::layer().with_writer(std::io::stderr));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().json().with_writer(writer))
        .with(console)
        .init();
    Some(guard)
}

/// Shows a native error box and exits; used before any window exists.
fn fail(message: &str) -> ! {
    tracing::error!(message, "cannot start");
    message_box(message);
    std::process::exit(1)
}

/// A native message box; works without WebView2.
fn message_box(message: &str) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
        let (body, title) = (wide(message), wide(text::APP_NAME));
        // SAFETY: both strings are NUL-terminated UTF-16 buffers that outlive the call.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                body.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            )
        };
    }
}

/// Windows 11 ships WebView2; some Windows 10 PCs lack it. Without it no window can open, but the Host, overlays
/// and tray keep running, so explain and open Microsoft's download page instead of quitting.
fn webview_available(app: &AppHandle) -> bool {
    if tauri::webview_version().is_ok() {
        return true;
    }
    tracing::error!(
        code = "desktop.webview_missing",
        "WebView2 Runtime not found"
    );
    message_box(text::WEBVIEW_MISSING);
    let _ = app.opener().open_url(WEBVIEW2_DOWNLOAD_URL, None::<&str>);
    false
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
        return Err("port.invalid");
    }
    let bound = match host::bind(host_config(&app, port)).await {
        Ok(bound) => bound,
        Err(HostError::PortInUse(_)) => return Err("port.in_use"),
        Err(_) => return Err("port.bind_failed"),
    };
    settings::update(&app.state::<AppState>().data_dir, |s| s.port = port)
        .map_err(|_| "settings.save_failed")?;
    start(&app, bound).map_err(|_| "port.start_failed")?;
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
        .map_err(|_| "admin.open_failed")
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
        data_dir: state.data_dir.clone(),
        twitch_secrets: state.twitch_secrets.clone(),
        twitch: TwitchEndpoints::default(),
    }
}

fn start(app: &AppHandle, bound: BoundHost) -> tauri::Result<()> {
    let port = bound.port();
    tracing::info!(port, "host listening");
    *app.state::<AppState>().host_port.lock().unwrap() = Some(port);
    tauri::async_runtime::spawn(async move {
        if let Err(e) = bound.serve().await {
            tracing::error!(code = "host.stopped", error = %e, "host stopped");
        }
    });
    open_admin_window(app, port)
}

fn webview_data_dir(app: &AppHandle) -> PathBuf {
    // Inside the data folder, so each copy has its own WebView2 profile.
    app.state::<AppState>().data_dir.join("WebView2")
}

fn open_admin_window(app: &AppHandle, port: u16) -> tauri::Result<()> {
    if !webview_available(app) {
        return Ok(());
    }
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
        .title(text::APP_NAME)
        .inner_size(1100.0, 720.0)
        .data_directory(webview_data_dir(app))
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
    if !webview_available(app) {
        return Ok(());
    }
    WebviewWindowBuilder::new(app, PORT_PAGE, WebviewUrl::App("port/index.html".into()))
        .title(text::APP_NAME)
        .inner_size(480.0, 320.0)
        .resizable(false)
        .data_directory(webview_data_dir(app))
        .build()?;
    Ok(())
}

fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", text::TRAY_OPEN, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", text::TRAY_QUIT, true, None::<&str>)?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip(text::APP_NAME)
        .menu(&Menu::with_items(app, &[&open, &quit])?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn show_main_window(app: &AppHandle) {
    let window = app
        .get_webview_window(ADMIN)
        .or_else(|| app.get_webview_window(PORT_PAGE));
    if let Some(w) = window {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    // No window yet (WebView2 was missing, or the port page was closed): create the one that fits now.
    let port = *app.state::<AppState>().host_port.lock().unwrap();
    let opened = match port {
        Some(port) => open_admin_window(app, port),
        None => open_port_page(app),
    };
    if let Err(e) = opened {
        tracing::error!(code = "desktop.window_failed", error = %e, "window not opened");
    }
}
