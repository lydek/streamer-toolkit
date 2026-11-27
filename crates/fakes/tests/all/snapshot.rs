//! What the control panel reads from the fakes and the few changes it makes, with the real clients connected.

use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::{Arc, atomic::AtomicU64},
    time::Duration,
};

use fakes::{
    control,
    metrics::{Direction, Fault, Recorder},
    obs::FakeObs,
    twitch::FakeTwitch,
};
use integrations::{
    obs::{Obs, ObsControl, ObsState, ObsStatus},
    secrets::InMemory,
    tls,
    twitch::{
        auth::{TwitchAuth, fake_stored_token},
        eventsub::EventSub,
        helix::Helix,
        irc::FirstChats,
    },
};
use serde_json::{Value, json};
use tokio::{net::TcpListener, sync::watch};
use toolkit_core::streaming::StreamingState;

struct Fakes {
    obs: FakeObs,
    twitch: FakeTwitch,
    obs_port: u16,
    port: u16,
}

async fn start() -> Fakes {
    let recorder = Recorder::default();
    let obs = FakeObs::with_recorder(None, recorder.clone());
    let twitch = FakeTwitch::with_recorder(Duration::from_secs(10), recorder);
    let obs_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let obs_port = obs_listener.local_addr().unwrap().port();
    tokio::spawn(obs.clone().serve(obs_listener));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = twitch
        .router()
        .merge(control::router(obs.clone(), twitch.clone()));
    tokio::spawn(async move { axum::serve(listener, app).await });
    Fakes {
        obs,
        twitch,
        obs_port,
        port,
    }
}

fn auth(port: u16) -> TwitchAuth {
    TwitchAuth::new(
        "fake-client".into(),
        &format!("http://127.0.0.1:{port}/oauth2"),
        tls::https_client(tls::client_config().unwrap()),
        Arc::new(InMemory::with(fake_stored_token())),
    )
}

fn obs_client(port: u16) -> (watch::Receiver<ObsStatus>, watch::Receiver<StreamingState>) {
    let (status_tx, status) = watch::channel(ObsStatus::CONNECTING);
    let (state_tx, state) = watch::channel(StreamingState::default());
    tokio::spawn(
        Obs {
            host: "127.0.0.1".into(),
            port,
            secrets: Arc::new(InMemory::default()),
            status: Arc::new(status_tx),
            state: Arc::new(state_tx),
            generation: 1,
            current: Arc::new(AtomicU64::new(1)),
            control: Arc::new(ObsControl::default()),
        }
        .run(),
    );
    (status, state)
}

fn eventsub(port: u16) -> watch::Receiver<bool> {
    let (up_tx, up) = watch::channel(false);
    let config = tls::client_config().unwrap();
    tokio::spawn(
        EventSub {
            url: format!("ws://127.0.0.1:{port}/eventsub"),
            helix: Helix::new(
                &format!("http://127.0.0.1:{port}/helix"),
                auth(port),
                tls::https_client(config.clone()),
            ),
            tls: config,
            deliver: Arc::new(|_| {}),
            deliver_alert: Arc::new(|_| {}),
            deliver_redemption: Arc::new(|_| {}),
            deliver_stream: Arc::new(|_| {}),
            deliver_contest: Arc::new(|_| {}),
            deliver_removal: Arc::new(|_, _| Box::pin(async {})),
            connected: Arc::new(move |c| {
                let _ = up_tx.send(c);
            }),
        }
        .run(),
    );
    up
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

async fn send(port: u16, body: Value) -> Value {
    tokio::task::spawn_blocking(move || {
        serde_json::from_str(&control::call(port, "/_control/send", &body).unwrap()).unwrap()
    })
    .await
    .unwrap()
}

async fn get(port: u16, path: &str) -> u16 {
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nClient-Id: fake-client\r\n\
         Authorization: Bearer fake-access\r\nConnection: close\r\n\r\n"
    );
    tokio::task::spawn_blocking(move || {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        s.read_to_string(&mut response).unwrap();
        response.split(' ').nth(1).unwrap().parse().unwrap()
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn open_connections_follow_the_clients() {
    let f = start().await;
    let (mut status, _) = obs_client(f.obs_port);
    let mut es = eventsub(f.port);
    let (irc_tx, mut irc) = watch::channel(false);
    tokio::spawn(
        FirstChats {
            url: format!("ws://127.0.0.1:{}/irc", f.port),
            auth: auth(f.port),
            tls: tls::client_config().unwrap(),
            deliver: Arc::new(|_| {}),
            connected: Arc::new(move |c| {
                let _ = irc_tx.send(c);
            }),
        }
        .run(),
    );
    tokio::time::timeout(
        Duration::from_secs(10),
        status.wait_for(|s| s.state == ObsState::Connected),
    )
    .await
    .unwrap()
    .unwrap();
    tokio::time::timeout(Duration::from_secs(10), es.wait_for(|u| *u))
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), irc.wait_for(|u| *u))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(f.obs.snapshot().open, 1);
    let t = f.twitch.snapshot();
    assert_eq!((t.eventsub_open, t.irc_open), (1, 1));
    assert_eq!(t.subscriptions.len(), 14);
    assert_eq!(t.irc_joins, ["fake_streamer"]);

    f.obs.drop_connections();
    until("the OBS drop", || f.obs.snapshot().open == 0).await;
    until("the OBS reconnect", || f.obs.snapshot().open == 1).await;
    f.twitch.drop_eventsub();
    until("the EventSub drop", || {
        f.twitch.snapshot().eventsub_open == 0
    })
    .await;
    until("the EventSub reconnect", || {
        f.twitch.snapshot().eventsub_open == 1
    })
    .await;
}

#[tokio::test]
async fn the_last_redemption_and_chat_are_the_ones_just_sent() {
    let f = start().await;
    // Nothing subscribed yet: the event is logged as not delivered.
    let early = send(f.port, json!({"kind": "cheer"})).await;
    let entries = f.twitch.recorder().recent(10);
    let cheer = entries
        .iter()
        .find(|e| e.key == "channel.cheer")
        .expect("logged");
    assert_eq!((cheer.direction, cheer.delivered), (Direction::Out, false));
    assert_eq!(cheer.id.as_deref(), early["id"].as_str());

    let mut es = eventsub(f.port);
    tokio::time::timeout(Duration::from_secs(10), es.wait_for(|u| *u))
        .await
        .unwrap()
        .unwrap();
    let redeem = send(f.port, json!({"kind": "redeem"})).await;
    let chat = send(f.port, json!({"kind": "chat", "message": "hi"})).await;
    let t = f.twitch.snapshot();
    assert_eq!(t.last_redemption.as_deref(), redeem["id"].as_str());
    assert_eq!(t.last_chat.as_deref(), chat["id"].as_str());
    let delivered = f
        .twitch
        .recorder()
        .recent(50)
        .into_iter()
        .find(|e| e.key == "channel.chat.message")
        .unwrap();
    assert!(delivered.delivered);
}

#[tokio::test]
async fn studio_mode_and_the_program_scene_change_from_obs_itself() {
    let f = start().await;
    let (mut status, mut state) = obs_client(f.obs_port);
    tokio::time::timeout(
        Duration::from_secs(10),
        status.wait_for(|s| s.state == ObsState::Connected),
    )
    .await
    .unwrap()
    .unwrap();
    f.obs.set_studio_mode(true);
    tokio::time::timeout(Duration::from_secs(5), state.wait_for(|s| s.studio_mode))
        .await
        .unwrap()
        .unwrap();
    f.obs.set_program("Break").unwrap();
    tokio::time::timeout(
        Duration::from_secs(5),
        state.wait_for(|s| s.scene.as_deref() == Some("Break")),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(f.obs.set_program("Nope").is_err());
    let snap = f.obs.snapshot();
    assert_eq!((snap.program.as_str(), snap.studio_mode), ("Break", true));
    assert_eq!(snap.scenes[1].items[0].source, "Logo", "items top first");
    assert!(snap.audio.iter().any(|a| a.name == "Mic"));
}

#[tokio::test]
async fn removing_one_fault_leaves_the_others() {
    let f = start().await;
    let until = std::time::Instant::now() + Duration::from_secs(60);
    for key in ["GET /helix/users", "GET /helix/streams"] {
        f.twitch.recorder().set_fault(
            key,
            Fault {
                status: Some(503),
                close: false,
                delay: Duration::ZERO,
                until,
            },
        );
    }
    assert!(f.twitch.recorder().remove_fault("GET /helix/users"));
    assert_eq!(get(f.port, "/helix/users").await, 200);
    assert_eq!(get(f.port, "/helix/streams").await, 503);
    assert_eq!(f.twitch.recorder().faults().len(), 1);
}
