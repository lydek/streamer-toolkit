use super::*;
use integrations::secrets::SecretStoreError;
use tower::ServiceExt;

mod backup_routes;
mod chat_routes;
mod feature_routes;
mod guard;
mod settings_routes;
mod sockets;
mod template_routes;
mod twitch_write_routes;

#[test]
fn the_fake_twitch_must_be_on_this_machine() {
    let fake = TwitchEndpoints::fake("127.0.0.1:4467").expect("this machine");
    assert_eq!(fake.oauth, "http://127.0.0.1:4467/oauth2");
    assert_eq!(fake.helix, "http://127.0.0.1:4467/helix");
    assert_eq!(fake.eventsub, "ws://127.0.0.1:4467/eventsub");
    assert_eq!(fake.irc, "ws://127.0.0.1:4467/irc");
    assert!(TwitchEndpoints::fake("localhost:4467").is_some());
    for bad in [
        "localhost.evil.com:4467",
        "evil.com:4467",
        "10.0.0.2:4467",
        "127.0.0.1",
        "127.0.0.1:0",
        "127.0.0.1:x",
        "http://127.0.0.1:4467",
        "",
    ] {
        assert!(TwitchEndpoints::fake(bad).is_none(), "{bad}");
    }
}

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
            irc: "ws://127.0.0.1:1".into(),
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

fn get_req(uri: &str, host: &str) -> axum::http::request::Builder {
    Request::builder().uri(uri).header(header::HOST, host)
}

async fn status(req: Request) -> StatusCode {
    app().oneshot(req).await.unwrap().status()
}
