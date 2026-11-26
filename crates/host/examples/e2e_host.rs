//! A headless Host for the UI E2E tests (`apps/web/playwright.config.ts`); never shipped.
//! Usage: `E2E_ADMIN_TOKEN=<random> e2e_host <port> <data-dir>`. Secrets live in memory only and Twitch
//! points at an address nothing listens on, so no real account or credential is ever touched.
//! With `E2E_FAKE_SERVICES=127.0.0.1:<port>` (the fake Twitch of `crates/fakes`) and `E2E_FAKE_OBS_PORT=<port>`,
//! it connects to the fake services instead, signed in with the fake token.

use std::{sync::Arc, time::Duration};

use host::{AlertTiming, CheckinTiming, HostConfig, TwitchEndpoints};
use integrations::{secrets::InMemory, twitch::auth::fake_stored_token};

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let usage = "usage: E2E_ADMIN_TOKEN=<token> e2e_host <port> <data-dir>";
    let port = args.next().and_then(|p| p.parse().ok()).expect(usage);
    let data_dir: std::path::PathBuf = args.next().expect(usage).into();
    let admin_token = std::env::var("E2E_ADMIN_TOKEN").expect(usage).into();
    let fake_twitch = std::env::var("E2E_FAKE_SERVICES").ok().map(|addr| {
        TwitchEndpoints::fake(&addr)
            .expect("E2E_FAKE_SERVICES must be 127.0.0.1:<port> or localhost:<port>")
    });
    let fake_obs_port = std::env::var("E2E_FAKE_OBS_PORT").ok().map(|p| {
        p.parse::<u16>()
            .expect("E2E_FAKE_OBS_PORT must be a port number")
    });
    let unreachable = "127.0.0.1:1";
    // Without the fake OBS, OBS points nowhere too, so an OBS running on this machine is never contacted.
    host::settings::update(&data_dir, |s| {
        s.obs.host = "127.0.0.1".into();
        s.obs.port = fake_obs_port.unwrap_or(1);
        if fake_twitch.is_some() && s.twitch_client_id.is_empty() {
            s.twitch_client_id = "fake-client".into();
        }
    })
    .expect("write settings.json");
    let twitch_secrets = match fake_twitch {
        Some(_) => InMemory::with(fake_stored_token()),
        None => InMemory::default(),
    };
    let db = host::db::open(&data_dir).await.expect("open the database");
    let host = host::bind(HostConfig {
        port,
        extra_origins: Vec::new(),
        admin_token,
        data_dir,
        twitch_secrets: Arc::new(twitch_secrets),
        twitch: fake_twitch.unwrap_or_else(|| TwitchEndpoints {
            oauth: format!("http://{unreachable}"),
            helix: format!("http://{unreachable}"),
            eventsub: format!("ws://{unreachable}"),
            irc: format!("ws://{unreachable}"),
        }),
        obs_secrets: Arc::new(InMemory::default()),
        // Short enough for a queue of alerts to play within a test, long enough to see each one.
        alert_timing: AlertTiming {
            show: Duration::from_millis(1500),
            show_with_message: Duration::from_millis(2000),
        },
        checkin_timing: CheckinTiming {
            show: Duration::from_millis(1500),
        },
        presentation_gap: Duration::from_millis(200),
        db,
        app_version: "0.0.0-e2e".into(),
        latest_release_url: None,
        restart: Arc::new(|| {}),
    })
    .await
    .expect("bind the E2E Host");
    println!("e2e host listening on 127.0.0.1:{}", host.port());
    host.serve().await.expect("serve");
}
