//! The control API and commands, checked with the real clients: each fake event reaches the matching EventSub
//! callback, and OBS commands reach the OBS client.

use std::{
    sync::{Arc, Mutex, atomic::AtomicU64},
    time::Duration,
};

use fakes::{control, events::KINDS, obs::FakeObs, twitch::FakeTwitch};
use integrations::{
    obs::{Obs, ObsControl, ObsState, ObsStatus},
    secrets::InMemory,
    tls,
    twitch::{
        auth::{TwitchAuth, fake_stored_token},
        eventsub::EventSub,
        helix::Helix,
    },
};
use serde_json::Value;
use tokio::{net::TcpListener, sync::watch};
use toolkit_core::streaming::StreamingState;

/// Which EventSub callback got each delivery, in order.
type Seen = Arc<Mutex<Vec<&'static str>>>;

struct Running {
    port: u16,
    seen: Seen,
    obs_status: watch::Receiver<ObsStatus>,
    obs: Arc<ObsControl>,
}

async fn start() -> Running {
    let fake_obs = FakeObs::new(None);
    let fake_twitch = FakeTwitch::new(Duration::from_secs(10));
    let obs_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let obs_port = obs_listener.local_addr().unwrap().port();
    tokio::spawn(fake_obs.clone().serve(obs_listener));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = fake_twitch
        .router()
        .merge(control::router(fake_obs, fake_twitch.clone()));
    tokio::spawn(async move { axum::serve(listener, app).await });

    let base = format!("127.0.0.1:{port}");
    let config = tls::client_config().unwrap();
    let http = tls::https_client(config.clone());
    let auth = TwitchAuth::new(
        "fake-client".into(),
        &format!("http://{base}/oauth2"),
        http.clone(),
        Arc::new(InMemory::with(fake_stored_token())),
    );
    let seen: Seen = Arc::default();
    let (up_tx, mut up) = watch::channel(false);
    let mark = |name: &'static str, seen: &Seen| {
        let seen = seen.clone();
        move || seen.lock().unwrap().push(name)
    };
    let (chat, alert, redemption, stream, removal) = (
        mark("chat", &seen),
        mark("alert", &seen),
        mark("redemption", &seen),
        mark("stream", &seen),
        mark("removal", &seen),
    );
    tokio::spawn(
        EventSub {
            url: format!("ws://{base}/eventsub"),
            helix: Helix::new(&format!("http://{base}/helix"), auth, http),
            tls: config,
            deliver: Arc::new(move |_| chat()),
            deliver_alert: Arc::new(move |_| alert()),
            deliver_redemption: Arc::new(move |_| redemption()),
            deliver_stream: Arc::new(move |_| stream()),
            deliver_contest: Arc::new(|_| {}),
            deliver_removal: Arc::new(move |_, _| {
                removal();
                Box::pin(async {})
            }),
            connected: Arc::new(move |connected| {
                let _ = up_tx.send(connected);
            }),
        }
        .run(),
    );
    tokio::time::timeout(Duration::from_secs(10), up.wait_for(|up| *up))
        .await
        .expect("EventSub connected")
        .unwrap();

    let (status_tx, obs_status) = watch::channel(ObsStatus::CONNECTING);
    let obs = Arc::new(ObsControl::default());
    tokio::spawn(
        Obs {
            host: "127.0.0.1".into(),
            port: obs_port,
            secrets: Arc::new(InMemory::default()),
            status: Arc::new(status_tx),
            state: Arc::new(watch::channel(StreamingState::default()).0),
            generation: 1,
            current: Arc::new(AtomicU64::new(1)),
            control: obs.clone(),
        }
        .run(),
    );
    Running {
        port,
        seen,
        obs_status,
        obs,
    }
}

/// Runs a command as `fake-services <words>` would, over HTTP.
async fn run(port: u16, words: &[&str]) -> Value {
    let words: Vec<String> = words.iter().map(|s| s.to_string()).collect();
    tokio::task::spawn_blocking(move || {
        let (path, body) = control::command(&words).unwrap();
        serde_json::from_str(&control::call(port, path, &body).unwrap()).unwrap()
    })
    .await
    .unwrap()
}

async fn until(what: &str, mut done: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
}

#[tokio::test]
async fn every_event_kind_reaches_its_eventsub_callback() {
    let r = start().await;
    for (kind, _) in KINDS {
        let expected = match *kind {
            "chat" => "chat",
            "delete" | "clear-user" | "clear" => "removal",
            "redeem" | "redeem-update" => "redemption",
            "online" | "offline" => "stream",
            _ => "alert",
        };
        let before = r.seen.lock().unwrap().len();
        let answer = run(r.port, &["send", kind, "--id", "r-1"]).await;
        assert_eq!(
            answer["skipped"],
            serde_json::json!([]),
            "{kind}: the app subscribed to it"
        );
        until(kind, || {
            r.seen.lock().unwrap()[before..].contains(&expected)
        })
        .await;
    }
}

#[tokio::test]
async fn a_gift_also_sends_one_sub_per_recipient() {
    let r = start().await;
    let answer = run(r.port, &["send", "gift", "--count", "3"]).await;
    let sent: Vec<&str> = answer["sent"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    assert_eq!(
        sent,
        [
            "channel.subscription.gift",
            "channel.subscribe",
            "channel.subscribe",
            "channel.subscribe"
        ]
    );
}

#[tokio::test]
async fn obs_commands_drop_the_connection_and_change_the_stats() {
    let mut r = start().await;
    tokio::time::timeout(
        Duration::from_secs(10),
        r.obs_status.wait_for(|s| s.state == ObsState::Connected),
    )
    .await
    .unwrap()
    .unwrap();

    run(r.port, &["obs", "stats", "--cpu", "77", "--dropped", "8"]).await;
    assert_eq!(r.obs.stats().await.unwrap().cpu_percent, 77.0);

    run(r.port, &["obs", "drop"]).await;
    tokio::time::timeout(
        Duration::from_secs(5),
        r.obs_status.wait_for(|s| s.state != ObsState::Connected),
    )
    .await
    .expect("the client saw the drop")
    .unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        r.obs_status.wait_for(|s| s.state == ObsState::Connected),
    )
    .await
    .expect("and connected again")
    .unwrap();
}

#[tokio::test]
async fn a_first_chat_also_arrives_over_irc() {
    let fake_twitch = FakeTwitch::new(Duration::from_secs(10));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = fake_twitch
        .router()
        .merge(control::router(FakeObs::new(None), fake_twitch.clone()));
    tokio::spawn(async move { axum::serve(listener, app).await });
    let firsts: Arc<Mutex<Vec<String>>> = Arc::default();
    let (up_tx, mut up) = watch::channel(false);
    let got = firsts.clone();
    tokio::spawn(
        integrations::twitch::irc::FirstChats {
            url: format!("ws://127.0.0.1:{port}/irc"),
            auth: TwitchAuth::new(
                "fake-client".into(),
                &format!("http://127.0.0.1:{port}/oauth2"),
                tls::https_client(tls::client_config().unwrap()),
                Arc::new(InMemory::with(fake_stored_token())),
            ),
            tls: tls::client_config().unwrap(),
            deliver: Arc::new(move |id| got.lock().unwrap().push(id)),
            connected: Arc::new(move |c| {
                let _ = up_tx.send(c);
            }),
        }
        .run(),
    );
    tokio::time::timeout(Duration::from_secs(10), up.wait_for(|up| *up))
        .await
        .unwrap()
        .unwrap();
    let answer = run(port, &["send", "chat", "--first", "--user", "newcomer"]).await;
    let id = answer["id"].as_str().unwrap().to_owned();
    until("the first-chat mark", || {
        firsts.lock().unwrap().contains(&id)
    })
    .await;
}
