// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod close_hint;
mod data_folder;
mod text;

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use host::{
    AlertTiming, BoundHost, ChatService, CheckinTiming, HostConfig, HostError, PRESENTATION_GAP,
    TwitchEndpoints, settings,
};
use integrations::{
    secrets::{CredentialManager, InMemory, SecretStore},
    twitch::auth::fake_stored_token,
};
use secrecy::{ExposeSecret, SecretString};
use tauri::{
    AppHandle, Emitter, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder, WindowEvent,
    ipc::CapabilityBuilder,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_opener::OpenerExt;
use tracing_appender::{non_blocking::WorkerGuard, rolling};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

const ADMIN: &str = "admin";
const PORT_PAGE: &str = "port";
/// Sent to the admin window: `true` shows the close hint, `false` closes it.
const CLOSE_HINT_EVENT: &str = "close-hint";
const TWITCH_ACTIVATE_URL: &str = "https://www.twitch.tv/activate";
/// The new-version notice's download page; never a URL from the release API's response.
const RELEASES_URL: &str = "https://github.com/lydek/streamer-toolkit/releases/latest";
const WEBVIEW2_DOWNLOAD_URL: &str = "https://developer.microsoft.com/microsoft-edge/webview2/";
const LOG_DAYS: usize = 14;
/// Release builds keep `host::DEFAULT_PORT` (47810). Must match apps/web/dev-server.ts (dev-server.test.ts checks).
const DEV_DEFAULT_PORT: u16 = 47811;
/// How long quitting waits for chat writes already accepted.
const EXIT_DRAIN: std::time::Duration = std::time::Duration::from_secs(10);

struct AppState {
    admin_token: SecretString,
    data_dir: PathBuf,
    twitch_secrets: Arc<dyn SecretStore>,
    obs_secrets: Arc<dyn SecretStore>,
    /// Twitch's own addresses, or the fake Twitch's in a dev build with `TOOLKIT_FAKE_SERVICES`.
    twitch: TwitchEndpoints,
    /// Set once the Host listens; lets the tray open the admin window later (e.g. after WebView2 is installed).
    host_port: Mutex<Option<u16>>,
    chat: Mutex<Option<Arc<ChatService>>>,
    exiting: AtomicBool,
    db: host::db::SqlitePool,
    close_hint: Mutex<close_hint::CloseHint>,
}

fn main() {
    let mut context = tauri::generate_context!();
    let base_identifier = context.config().identifier.clone();
    let data_dir = data_folder::resolve()
        .unwrap_or_else(|e| fail(&text::data_folder_unusable("", &e.to_string())));
    // Before anything is written: a refused folder must not even get logs.
    let unusable = |e: std::io::Error| -> ! {
        fail(&text::data_folder_unusable(
            &data_dir.display().to_string(),
            &e.to_string(),
        ))
    };
    match data_folder::dev_access(
        cfg!(dev),
        data_folder::has_flag("--data-dir"),
        data_folder::has_flag(data_folder::ALLOW_RELEASE_DATA),
        || data_folder::folder_state(&data_dir),
    ) {
        Ok(data_folder::DevAccess::Allow) => {}
        Ok(data_folder::DevAccess::Mark) => {
            data_folder::mark_dev(&data_dir).unwrap_or_else(|e| unusable(e))
        }
        Ok(data_folder::DevAccess::Refuse) => fail(&text::dev_refuses_folder(
            &data_dir.display().to_string(),
            data_folder::ALLOW_RELEASE_DATA,
        )),
        Err(e) => unusable(e),
    }
    data_folder::ensure_writable(&data_dir).unwrap_or_else(|e| unusable(e));
    let mut log_guard = init_logs(&data_dir.join("logs"));
    let instance = data_folder::instance_id(&data_dir).unwrap_or_else(|e| unusable(e));
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
            // After the single-instance lock, so a second copy on this folder never migrates under the first;
            // a confirmed restore goes in before the database opens.
            let version = app.package_info().version.to_string();
            let notice = host::restore_pending(&data_dir).then(restore_notice);
            let applied = tauri::async_runtime::block_on(host::apply_pending(&data_dir, &version));
            if let Some(close) = notice {
                close();
            }
            match applied {
                Ok(false) => {}
                Ok(true) => message_box(text::RESTORED, false),
                Err(e @ host::BackupError::PutBack(_)) => {
                    fail(&text::restore_put_back_failed(&data_dir.display().to_string(), &e.to_string()))
                }
                Err(e @ host::BackupError::MarkerLeft(_)) => {
                    tracing::error!(code = "backup.marker_left", error = %e, "restore marker left behind");
                    message_box(&text::restore_marker_left(&data_dir.display().to_string(), &e.to_string()), true);
                }
                Err(e) => {
                    tracing::error!(code = "backup.restore_failed", error = %e, "restore not applied");
                    message_box(&text::restore_failed(&e.to_string()), true);
                }
            }
            let db = match tauri::async_runtime::block_on(host::db::open(&data_dir)) {
                Ok(db) => db,
                Err(e) => fail(&text::database_unusable(
                    &data_dir.join(host::db::DB_FILE).display().to_string(),
                    &e.to_string(),
                )),
            };
            // Release builds never read the variable.
            let fake = cfg!(dev)
                .then(|| std::env::var(FAKE_SERVICES).ok())
                .flatten();
            let twitch = fake_services(cfg!(dev), fake.as_deref());
            if fake.is_some() && twitch.is_none() {
                tracing::warn!(code = "dev.fake_services_ignored", "{FAKE_SERVICES} must be 127.0.0.1:<port> or localhost:<port>");
            }
            // The fake Twitch's tokens and the fake OBS password stay in memory, never in the Credential Manager.
            let (twitch_secrets, obs_secrets): (Arc<dyn SecretStore>, Arc<dyn SecretStore>) =
                if twitch.is_some() {
                    tracing::warn!(code = "dev.fake_services", "using the fake Twitch; secrets stay in memory");
                    (
                        Arc::new(InMemory::with(fake_stored_token())),
                        Arc::new(InMemory::default()),
                    )
                } else {
                    (
                        CredentialManager::new(&format!("{instance}/twitch"))?,
                        CredentialManager::new(&format!("{instance}/obs"))?,
                    )
                };
            app.manage(AppState {
                admin_token: random_token()?,
                data_dir: data_dir.clone(),
                twitch_secrets,
                obs_secrets,
                twitch: twitch.unwrap_or_default(),
                host_port: Mutex::new(None),
                chat: Mutex::default(),
                exiting: AtomicBool::default(),
                db,
                close_hint: Mutex::default(),
            });
            create_tray(app.handle())?;
            let handle = app.handle().clone();
            // A folder without settings has no OBS URL yet, so a taken default port can be swapped for a free one.
            let first_run = !settings::path(&data_dir).exists();
            let port = start_port(cfg!(dev), first_run, || settings::load(&data_dir).port);
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
            open_twitch_activate,
            open_download_page,
            open_twitch_chat,
            open_logs_folder,
            close_hint
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
            admin_close_requested(app);
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

/// The fake Twitch of `crates/fakes` (`pnpm dev:fake`), as `<host>:<port>`; read by dev builds only.
const FAKE_SERVICES: &str = "TOOLKIT_FAKE_SERVICES";

/// The fake Twitch's addresses for `TOOLKIT_FAKE_SERVICES`: dev builds only, and only on this machine.
fn fake_services(is_dev: bool, value: Option<&str>) -> Option<TwitchEndpoints> {
    value.filter(|_| is_dev).and_then(TwitchEndpoints::fake)
}

/// The port the first try binds. A new folder of a dev build starts on `DEV_DEFAULT_PORT`, so a dev build never
/// takes the port OBS browser sources point at. A `settings.json` written by a script makes the folder not new:
/// such scripts must write `port` themselves, or the saved default (`host::DEFAULT_PORT`) applies.
fn start_port(is_dev: bool, first_run: bool, saved: impl FnOnce() -> u16) -> u16 {
    if is_dev && first_run {
        DEV_DEFAULT_PORT
    } else {
        saved()
    }
}

/// JSON lines, one file per day, the last `LOG_DAYS` kept. Dev builds also print to stderr.
fn init_logs(dir: &Path) -> Option<WorkerGuard> {
    // tracing-appender prunes old files before it creates the folder, which prints an error on first start.
    std::fs::create_dir_all(dir).ok()?;
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
    message_box(message, true);
    std::process::exit(1)
}

/// A native message box; works without WebView2.
fn message_box(message: &str, is_error: bool) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MessageBoxW,
        };
        let (body, title) = (wide(message), wide(text::APP_NAME));
        // SAFETY: both strings are NUL-terminated UTF-16 buffers that outlive the call.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                body.as_ptr(),
                title.as_ptr(),
                MB_OK
                    | if is_error {
                        MB_ICONERROR
                    } else {
                        MB_ICONINFORMATION
                    },
            )
        };
    }
}

/// While a confirmed restore backs up the current data and swaps it in, which takes a while for a large folder and
/// runs before any window exists, a native notice says so. Returns what closes it; its OK button only hides it early.
#[cfg(windows)]
fn restore_notice() -> impl FnOnce() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::{Duration, Instant};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, GetWindowThreadProcessId, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND,
        MessageBoxW, PostMessageW, WM_CLOSE,
    };
    let done = Arc::new(AtomicBool::new(false));
    let shown = std::thread::spawn({
        let done = done.clone();
        move || {
            // A restore that ends this soon needs no notice: skipping it avoids a flash before the result box. Closing
            // wakes this wait early; parking may also wake by itself, hence the loop.
            let until = Instant::now() + Duration::from_millis(150);
            while !done.load(Ordering::SeqCst) && Instant::now() < until {
                std::thread::park_timeout(until.saturating_duration_since(Instant::now()));
            }
            if done.load(Ordering::SeqCst) {
                return;
            }
            let (body, title) = (wide(text::RESTORING), wide(text::RESTORING_TITLE));
            // SAFETY: both strings are NUL-terminated UTF-16 buffers that outlive the call.
            unsafe {
                MessageBoxW(
                    std::ptr::null_mut(),
                    body.as_ptr(),
                    title.as_ptr(),
                    MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND,
                )
            };
        }
    });
    move || {
        done.store(true, Ordering::SeqCst);
        shown.thread().unpark();
        // Message boxes are dialogs (#32770); only this process's one with that title is closed, never another copy's
        // notice or another program's window.
        let (class, title) = (wide("#32770"), wide(text::RESTORING_TITLE));
        let me = std::process::id();
        // The box may still be opening: look for it until its thread ends (WM_CLOSE answers it like OK). Startup never
        // waits on it for long: past the limit the box stays until its OK, and starting goes on.
        let give_up = Instant::now() + Duration::from_secs(2);
        while !shown.is_finished() && Instant::now() < give_up {
            let mut window = std::ptr::null_mut();
            loop {
                // SAFETY: class and title are NUL-terminated UTF-16 buffers; `window` is null or a handle this
                // search returned, as FindWindowExW expects for its "after" argument.
                window = unsafe {
                    FindWindowExW(std::ptr::null_mut(), window, class.as_ptr(), title.as_ptr())
                };
                if window.is_null() {
                    break;
                }
                let mut pid = 0;
                // SAFETY: `window` is a handle FindWindowExW just returned; `pid` is a valid out pointer.
                unsafe { GetWindowThreadProcessId(window, &mut pid) };
                if pid == me {
                    // SAFETY: posting WM_CLOSE to a window of this process found above.
                    unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// A NUL-terminated UTF-16 copy of `s`, for Win32 calls.
#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

#[cfg(not(windows))]
fn restore_notice() -> impl FnOnce() {
    || {}
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
    message_box(text::WEBVIEW_MISSING, true);
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
/// 開啟記錄檔資料夾: only this data folder's `logs/`; it takes no path, so the page cannot name another folder.
#[tauri::command]
fn open_logs_folder(app: AppHandle, state: State<'_, AppState>) -> Result<(), &'static str> {
    let logs = state.data_dir.join("logs");
    app.opener()
        .open_path(logs.to_string_lossy(), None::<&str>)
        .map_err(|_| "admin.open_failed")
}

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

/// Opens the fixed releases page in the system browser for the new-version notice; takes no URL.
#[tauri::command]
fn open_download_page(app: AppHandle) -> Result<(), &'static str> {
    app.opener()
        .open_url(RELEASES_URL, None::<&str>)
        .map_err(|_| "admin.open_failed")
}

/// Twitch's pop-out chat of `login`; `None` unless `login` is a Twitch login (letters, digits, `_`, up to 25), so
/// the URL never leaves this fixed form.
fn twitch_chat_url(login: &str) -> Option<String> {
    let valid = (1..=25).contains(&login.len())
        && login
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    valid.then(|| format!("https://www.twitch.tv/popout/{login}/chat"))
}

/// Opens the broadcaster's Twitch chat in the system browser (the chat log page's 在 Twitch 開啟); takes only a
/// login, never a URL.
#[tauri::command]
fn open_twitch_chat(app: AppHandle, login: String) -> Result<(), &'static str> {
    let url = twitch_chat_url(&login).ok_or("admin.invalid_login")?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|_| "admin.open_failed")
}

/// The admin window reports what happened to the close hint.
#[tauri::command]
fn close_hint(app: AppHandle, state: State<'_, AppState>, action: close_hint::Action) {
    let hide = state.close_hint.lock().unwrap().act(action);
    if let Some(dont_ask_again) = hide {
        if dont_ask_again
            && let Err(e) = settings::update(&state.data_dir, |s| s.skip_close_hint = true)
        {
            tracing::error!(code = "settings.save_failed", error = %e, "close hint choice not saved");
        }
        hide_admin_window(&app);
    }
}

/// ✕ on the admin window: hide it, or first ask the window to explain the tray (docs/architecture.md §6).
fn admin_close_requested(app: &AppHandle) {
    let state = app.state::<AppState>();
    let skip_hint = settings::load(&state.data_dir).skip_close_hint;
    let Some(id) = state.close_hint.lock().unwrap().close_requested(skip_hint) else {
        return hide_admin_window(app);
    };
    // Closing from the taskbar does not restore a minimized window; the hint must be visible to be answered.
    if let Some(w) = app.get_webview_window(ADMIN) {
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    if app.emit_to(ADMIN, CLOSE_HINT_EVENT, true).is_err() {
        state
            .close_hint
            .lock()
            .unwrap()
            .act(close_hint::Action::Cancel);
        return hide_admin_window(app);
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(close_hint::TIMEOUT);
        if app
            .state::<AppState>()
            .close_hint
            .lock()
            .unwrap()
            .timed_out(id)
        {
            hide_admin_window(&app);
        }
    });
}

/// Hides the admin window and closes a hint still open in it, so it is gone when the window comes back.
fn hide_admin_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(ADMIN) {
        let _ = w.hide();
    }
    let _ = app.emit_to(ADMIN, CLOSE_HINT_EVENT, false);
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
        twitch: state.twitch.clone(),
        obs_secrets: state.obs_secrets.clone(),
        alert_timing: AlertTiming::default(),
        checkin_timing: CheckinTiming::default(),
        presentation_gap: PRESENTATION_GAP,
        db: state.db.clone(),
        app_version: app.package_info().version.to_string(),
        // A dev build's version says nothing about releases, and its admin window may not open the download page.
        latest_release_url: (!cfg!(dev)).then(|| integrations::releases::LATEST_RELEASE_URL.into()),
        restart: Arc::new({
            let app = app.clone();
            move || finish_with_chat(&app, true)
        }),
    }
}

fn start(app: &AppHandle, bound: BoundHost) -> tauri::Result<()> {
    let port = bound.port();
    tracing::info!(port, "host listening");
    *app.state::<AppState>().host_port.lock().unwrap() = Some(port);
    *app.state::<AppState>().chat.lock().unwrap() = Some(bound.chat());
    tauri::async_runtime::spawn(async move {
        if let Err(e) = bound.serve().await {
            tracing::error!(code = "host.stopped", error = %e, "host stopped");
        }
    });
    open_admin_window(app, port)
}

fn finish_with_chat(app: &AppHandle, restart: bool) {
    let state = app.state::<AppState>();
    if state.exiting.swap(true, Ordering::Relaxed) {
        return;
    }
    let chat = state.chat.lock().unwrap().clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(chat) = chat {
            tracing::info!("waiting for pending chat writes before exit");
            // Storage retries a failing database forever; quitting must still quit.
            if !chat.drain(EXIT_DRAIN).await {
                tracing::error!(
                    missed = chat.missed_writes(),
                    "chat writes still pending at exit; leaving them unsaved"
                );
            }
        }
        if restart {
            app.request_restart();
        } else {
            app.exit(0);
        }
    });
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
        // `admin/`, not `admin/index.html`: the router's base is /admin/, so the latter would be the route /index.html.
        WebviewUrl::App("admin/".into())
    } else {
        // Release: the Host URL is a remote origin, so its capability is created for the bound port only.
        // Tauri matches IPC calls by their Origin header, which has no path; on_navigation keeps the window on /admin.
        app.add_capability(
            CapabilityBuilder::new("admin-remote")
                .remote(format!("http://127.0.0.1:{port}"))
                .window(ADMIN)
                .permission("allow-get-admin-token")
                .permission("allow-open-twitch-activate")
                .permission("allow-open-download-page")
                .permission("allow-open-twitch-chat")
                .permission("allow-open-logs-folder")
                .permission("allow-close-hint")
                // Receives the close hint event; no core:default or core:window:*, the capability covers the whole Host origin.
                .permission("core:event:allow-listen")
                .permission("core:event:allow-unlisten"),
        )?;
        let host_url = format!("http://127.0.0.1:{port}/admin");
        WebviewUrl::External(host_url.parse().expect("valid admin URL"))
    };
    let allowed_origin = if cfg!(dev) {
        app.config()
            .build
            .dev_url
            .as_ref()
            .map(|u| u.origin().ascii_serialization())
            .unwrap_or_default()
    } else {
        format!("http://127.0.0.1:{port}")
    };
    WebviewWindowBuilder::new(app, ADMIN, url)
        .title(text::APP_NAME)
        .inner_size(1100.0, 720.0)
        .min_inner_size(360.0, 480.0)
        // Ctrl +/-/wheel: lets users enlarge the UI (WCAG 1.4.4).
        .zoom_hotkeys_enabled(true)
        .data_directory(webview_data_dir(app))
        .on_navigation(move |url| admin_navigation_allowed(url, &allowed_origin))
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .build()?;
    Ok(())
}

/// Only /admin pages on the app's own origin: the Host's, or in dev the Vite origin (devUrl).
fn admin_navigation_allowed(url: &tauri::Url, origin: &str) -> bool {
    url.origin().ascii_serialization() == origin
        && (url.path() == "/admin" || url.path().starts_with("/admin/"))
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
            "quit" => finish_with_chat(app, false),
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

#[cfg(test)]
mod tests {
    #[test]
    fn fake_services_need_a_dev_build() {
        use super::fake_services;
        let fake =
            fake_services(true, Some("127.0.0.1:4467")).expect("a dev build on this machine");
        assert_eq!(fake.helix, "http://127.0.0.1:4467/helix");
        assert!(
            fake_services(false, Some("127.0.0.1:4467")).is_none(),
            "release builds ignore it"
        );
        assert!(
            fake_services(true, Some("localhost.evil.com:4467")).is_none(),
            "this machine only"
        );
        assert!(fake_services(true, None).is_none());
    }

    #[test]
    fn only_a_new_folder_of_a_dev_build_starts_on_the_dev_port() {
        use super::{DEV_DEFAULT_PORT, start_port};
        assert_eq!(start_port(true, true, || unreachable!()), DEV_DEFAULT_PORT);
        assert_eq!(start_port(true, false, || 47810), 47810);
        assert_eq!(
            start_port(false, true, || host::DEFAULT_PORT),
            host::DEFAULT_PORT
        );
        assert_eq!(start_port(false, false, || 51234), 51234);
        assert_ne!(DEV_DEFAULT_PORT, host::DEFAULT_PORT);
    }

    #[test]
    fn the_twitch_chat_url_takes_only_a_login() {
        assert_eq!(
            super::twitch_chat_url("streamer_tw01").as_deref(),
            Some("https://www.twitch.tv/popout/streamer_tw01/chat")
        );
        for bad in [
            "",
            "Streamer",
            "a/b",
            "a?b=c",
            "evil.com",
            "x".repeat(26).as_str(),
            "../admin",
        ] {
            assert_eq!(super::twitch_chat_url(bad), None, "{bad}");
        }
    }

    use super::admin_navigation_allowed;

    #[test]
    fn the_admin_window_stays_on_its_own_admin_pages() {
        let allowed =
            |url: &str| admin_navigation_allowed(&url.parse().unwrap(), "http://127.0.0.1:47810");
        assert!(allowed("http://127.0.0.1:47810/admin"));
        assert!(allowed("http://127.0.0.1:47810/admin/settings#backup"));
        assert!(!allowed("http://127.0.0.1:47810/overlay/chat"));
        assert!(!allowed("http://127.0.0.1:47810/administrator"));
        assert!(!allowed("http://127.0.0.1:47811/admin/"));
        assert!(!allowed("http://localhost:5173/admin/"));
        assert!(!allowed("https://evil.example/admin/"));
        assert!(!allowed("data:text/html,<script>alert(1)</script>"));
        // Dev: only the Vite origin.
        assert!(admin_navigation_allowed(
            &"http://localhost:5173/admin/".parse().unwrap(),
            "http://localhost:5173"
        ));
        assert!(!admin_navigation_allowed(
            &"http://127.0.0.1:47810/admin/".parse().unwrap(),
            "http://localhost:5173"
        ));
    }
}
