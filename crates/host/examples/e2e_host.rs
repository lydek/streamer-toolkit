//! A headless Host for the UI E2E tests (`apps/web/playwright.config.ts`); never shipped.
//! Usage: `E2E_ADMIN_TOKEN=<random> e2e_host <port> <data-dir>`. Secrets live in memory only and Twitch
//! points at an address nothing listens on, so no real account or credential is ever touched.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use host::{AlertTiming, CheckinTiming, HostConfig, TwitchEndpoints};
use integrations::secrets::{SecretStore, SecretStoreError};

#[derive(Default)]
struct MemorySecret(Mutex<Option<String>>);

impl SecretStore for MemorySecret {
    fn read(&self) -> Result<Option<String>, SecretStoreError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn write(&self, value: &str) -> Result<(), SecretStoreError> {
        *self.0.lock().unwrap() = Some(value.into());
        Ok(())
    }
    fn delete(&self) -> Result<(), SecretStoreError> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let usage = "usage: E2E_ADMIN_TOKEN=<token> e2e_host <port> <data-dir>";
    let port = args.next().and_then(|p| p.parse().ok()).expect(usage);
    let data_dir: std::path::PathBuf = args.next().expect(usage).into();
    let admin_token = std::env::var("E2E_ADMIN_TOKEN").expect(usage).into();
    let unreachable = "127.0.0.1:1";
    // OBS points nowhere too, so an OBS running on this machine is never contacted.
    host::settings::update(&data_dir, |s| {
        s.obs.host = "127.0.0.1".into();
        s.obs.port = 1;
    })
    .expect("write settings.json");
    let db = host::db::open(&data_dir).await.expect("open the database");
    let host = host::bind(HostConfig {
        port,
        extra_origins: Vec::new(),
        admin_token,
        data_dir,
        twitch_secrets: Arc::new(MemorySecret::default()),
        twitch: TwitchEndpoints {
            oauth: format!("http://{unreachable}"),
            helix: format!("http://{unreachable}"),
            eventsub: format!("ws://{unreachable}"),
        },
        obs_secrets: Arc::new(MemorySecret::default()),
        // Short enough for a queue of alerts to play within a test, long enough to see each one.
        alert_timing: AlertTiming {
            show: Duration::from_millis(1500),
            show_with_message: Duration::from_millis(2000),
            gap: Duration::from_millis(200),
        },
        checkin_timing: CheckinTiming {
            show: Duration::from_millis(1500),
            gap: Duration::from_millis(200),
        },
        db,
        app_version: "0.0.0-e2e".into(),
    })
    .await
    .expect("bind the E2E Host");
    println!("e2e host listening on 127.0.0.1:{}", host.port());
    host.serve().await.expect("serve");
}
