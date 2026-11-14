fn main() {
    // App commands must be declared so each one needs an explicit capability grant.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "retry_with_port",
            "get_admin_token",
            "open_twitch_activate",
            "close_hint",
        ]),
    ))
    .expect("failed to run tauri-build");
}
