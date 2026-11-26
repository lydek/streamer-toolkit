//! The real Twitch clients (`integrations::twitch`) against the fake Twitch.

use std::{sync::Arc, time::Duration};

use fakes::twitch::{BROADCASTER_ID, BROADCASTER_LOGIN, FakeTwitch};
use integrations::{
    secrets::InMemory,
    tls,
    twitch::{
        MVP_SCOPES,
        auth::{AuthStatus, FAKE_LOGIN, FAKE_USER_ID, TwitchAuth, fake_stored_token},
        eventsub::EventSub,
        helix::Helix,
        irc::FirstChats,
    },
};
use tokio::{net::TcpListener, sync::mpsc};

async fn start(fake: FakeTwitch) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(fake.serve(listener));
    base
}

fn signed_in(base: &str) -> TwitchAuth {
    let http = tls::https_client(tls::client_config().unwrap());
    let store = Arc::new(InMemory::with(fake_stored_token()));
    TwitchAuth::new(
        "fake-client".into(),
        &format!("http://{base}/oauth2"),
        http,
        store,
    )
}

async fn until(what: &str, mut done: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
}

#[test]
fn the_fake_broadcaster_matches_the_dev_builds_token() {
    assert_eq!(
        (BROADCASTER_ID, BROADCASTER_LOGIN),
        (FAKE_USER_ID, FAKE_LOGIN)
    );
}

#[tokio::test]
async fn the_device_login_is_approved_at_once() {
    let base = start(FakeTwitch::default()).await;
    let http = tls::https_client(tls::client_config().unwrap());
    let auth = TwitchAuth::new(
        "fake-client".into(),
        &format!("http://{base}/oauth2"),
        http,
        Arc::new(InMemory::default()),
    );
    let login = auth.start_login(MVP_SCOPES).await.unwrap();
    assert_eq!(login.user_code, "FAKE-CODE");
    until("the sign-in", || {
        matches!(auth.status(), AuthStatus::SignedIn { .. })
    })
    .await;
    assert_eq!(
        auth.status(),
        AuthStatus::SignedIn {
            user_id: BROADCASTER_ID.into(),
            login: BROADCASTER_LOGIN.into()
        },
        "every scope granted"
    );
}

#[tokio::test]
async fn helix_answers_users_without_avatars() {
    let base = start(FakeTwitch::default()).await;
    let auth = signed_in(&base);
    let http = tls::https_client(tls::client_config().unwrap());
    let helix = Helix::new(&format!("http://{base}/helix"), auth, http);
    let me = helix.get_json("/users").await.unwrap();
    assert_eq!(me["data"][0]["login"], BROADCASTER_LOGIN);
    assert_eq!(me["data"][0]["profile_image_url"], "");
    let viewers = helix.get_json("/users?id=7&login=viewer_a").await.unwrap();
    assert_eq!(viewers["data"].as_array().unwrap().len(), 2);
    let rewards = helix
        .get_json(&format!(
            "/channel_points/custom_rewards?broadcaster_id={BROADCASTER_ID}"
        ))
        .await
        .unwrap();
    assert!(!rewards["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn eventsub_subscribes_and_keepalives_hold_the_session() {
    let fake = FakeTwitch::new(Duration::from_secs(2));
    let base = start(fake.clone()).await;
    let auth = signed_in(&base);
    let config = tls::client_config().unwrap();
    let helix = Helix::new(
        &format!("http://{base}/helix"),
        auth,
        tls::https_client(config.clone()),
    );
    let (up_tx, mut up) = mpsc::unbounded_channel();
    tokio::spawn(
        EventSub {
            url: format!("ws://{base}/eventsub"),
            helix,
            tls: config,
            deliver: Arc::new(|_| {}),
            deliver_alert: Arc::new(|_| {}),
            deliver_redemption: Arc::new(|_| {}),
            deliver_stream: Arc::new(|_| {}),
            deliver_removal: Arc::new(|_, _| Box::pin(async {})),
            connected: Arc::new(move |up| {
                let _ = up_tx.send(up);
            }),
        }
        .run(),
    );
    let connected = tokio::time::timeout(Duration::from_secs(10), up.recv())
        .await
        .unwrap();
    assert_eq!(connected, Some(true));
    let kinds: Vec<String> = fake
        .subscriptions()
        .iter()
        .filter_map(|s| s["type"].as_str().map(str::to_owned))
        .collect();
    for kind in [
        "channel.chat.message",
        "channel.follow",
        "channel.raid",
        "stream.online",
    ] {
        assert!(kinds.iter().any(|k| k == kind), "{kind} in {kinds:?}");
    }
    // Longer than the 2 s keepalive timeout: the keepalives keep the one session.
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert_eq!(fake.eventsub_connects(), 1);
}

#[tokio::test]
async fn irc_joins_the_broadcasters_channel() {
    let fake = FakeTwitch::new(Duration::from_secs(2));
    let base = start(fake.clone()).await;
    let (up_tx, mut up) = mpsc::unbounded_channel();
    tokio::spawn(
        FirstChats {
            url: format!("ws://{base}/irc"),
            auth: signed_in(&base),
            tls: tls::client_config().unwrap(),
            deliver: Arc::new(|_| {}),
            connected: Arc::new(move |up| {
                let _ = up_tx.send(up);
            }),
        }
        .run(),
    );
    let connected = tokio::time::timeout(Duration::from_secs(10), up.recv())
        .await
        .unwrap();
    assert_eq!(connected, Some(true));
    assert_eq!(fake.irc_joins(), [BROADCASTER_LOGIN]);
}
