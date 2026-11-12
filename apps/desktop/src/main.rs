// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod settings;

use host::{BoundHost, HostConfig, HostError};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const ADMIN: &str = "admin";
const PORT_PAGE: &str = "port";

fn main() {
    tauri::Builder::default()
        // Registered first: a second instance hands off before it ever tries to bind the port.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            focus_main_window(app)
        }))
        .setup(|app| {
            let handle = app.handle().clone();
            let port = settings::load(&handle).port;
            match tauri::async_runtime::block_on(host::bind(host_config(&handle, port))) {
                Ok(bound) => start(&handle, bound, port)?,
                Err(HostError::PortInUse(_)) => open_port_page(&handle)?,
                Err(e) => return Err(e.into()),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![retry_with_port])
        .run(tauri::generate_context!())
        .expect("error while running Streamer Toolkit");
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
    settings::save(&app, &settings::Settings { port }).map_err(|_| "save_failed")?;
    start(&app, bound, port).map_err(|_| "start_failed")?;
    if let Some(w) = app.get_webview_window(PORT_PAGE) {
        let _ = w.close();
    }
    Ok(())
}

fn host_config(app: &AppHandle, port: u16) -> HostConfig {
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
        // Dev: served by Vite through devUrl (local origin).
        WebviewUrl::App("admin/index.html".into())
    } else {
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
