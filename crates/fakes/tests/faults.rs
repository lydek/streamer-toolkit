//! Faults: failing or slow calls and refused connections, and what the stats say about the retries.

use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::{Arc, atomic::AtomicU64},
    time::{Duration, Instant},
};

use fakes::{control, metrics::Recorder, obs::FakeObs, twitch::FakeTwitch};
use integrations::{
    obs::{Obs, ObsControl, ObsState, ObsStatus},
    secrets::InMemory,
};
use serde_json::Value;
use tokio::{net::TcpListener, sync::watch};
use toolkit_core::streaming::StreamingState;

struct Fakes {
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
    let app = twitch.router().merge(control::router(obs, twitch));
    tokio::spawn(async move { axum::serve(listener, app).await });
    Fakes { obs_port, port }
}

async fn control(port: u16, words: &[&str]) -> Value {
    let words: Vec<String> = words.iter().map(|s| s.to_string()).collect();
    tokio::task::spawn_blocking(move || {
        let (path, body) = control::command(&words).unwrap();
        serde_json::from_str(&control::call(port, path, &body).unwrap()).unwrap()
    })
    .await
    .unwrap()
}

/// A Helix GET with valid headers; the status code.
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

fn warnings_for(report: &Value, kind: &str, key: &str) -> usize {
    report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|w| w["kind"] == kind && w["key"] == key)
        .count()
}

#[tokio::test]
async fn a_fault_fails_or_slows_a_call_until_it_ends() {
    let f = start().await;
    control(
        f.port,
        &[
            "fault",
            "GET /helix/users",
            "--status",
            "500",
            "--for",
            "800ms",
        ],
    )
    .await;
    assert_eq!(get(f.port, "/helix/users").await, 500);
    assert_eq!(
        get(f.port, "/helix/streams").await,
        200,
        "other calls are untouched"
    );
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert_eq!(get(f.port, "/helix/users").await, 200, "the fault ended");

    control(
        f.port,
        &[
            "fault",
            "GET /helix/users",
            "--delay",
            "300ms",
            "--for",
            "5s",
        ],
    )
    .await;
    let started = Instant::now();
    assert_eq!(get(f.port, "/helix/users").await, 200);
    assert!(started.elapsed() >= Duration::from_millis(300));
    control(f.port, &["fault", "--clear"]).await;
    let started = Instant::now();
    get(f.port, "/helix/users").await;
    assert!(started.elapsed() < Duration::from_millis(300), "cleared");
}

#[tokio::test]
async fn steady_retries_are_flagged_and_doubling_ones_are_not() {
    let f = start().await;
    for key in ["GET /helix/users", "GET /helix/streams"] {
        control(f.port, &["fault", key, "--status", "503", "--for", "1m"]).await;
    }
    // Retrying every 150 ms.
    for _ in 0..5 {
        assert_eq!(get(f.port, "/helix/streams").await, 503);
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    // Waiting twice as long each time. Each gap also holds the request's round trip, so the waits must stay above it
    // for a gap to grow over 1.5x: starting at 300 ms keeps this true on machines whose loopback adds ~200 ms.
    for wait in [300, 600, 1200, 2400, 0] {
        assert_eq!(get(f.port, "/helix/users").await, 503);
        tokio::time::sleep(Duration::from_millis(wait)).await;
    }

    let report = control(f.port, &["stats"]).await;
    assert_eq!(
        warnings_for(&report, "retry_backoff", "GET /helix/streams"),
        1,
        "{report}"
    );
    assert_eq!(
        warnings_for(&report, "retry_backoff", "GET /helix/users"),
        0,
        "{report}"
    );
    let users = report["retries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["key"] == "GET /helix/users")
        .expect("the retries are listed");
    assert_eq!(users["retries"], 4);
    let gaps: Vec<f64> = users["gaps_ms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g.as_f64().unwrap())
        .collect();
    assert_eq!(gaps.len(), 4);
    assert!(gaps.windows(2).all(|w| w[1] > w[0] * 1.5), "{gaps:?}");
}

#[tokio::test]
async fn the_obs_client_backs_off_while_connections_are_refused() {
    let f = start().await;
    let (status_tx, mut status) = watch::channel(ObsStatus::CONNECTING);
    tokio::spawn(
        Obs {
            host: "127.0.0.1".into(),
            port: f.obs_port,
            secrets: Arc::new(InMemory::default()),
            status: Arc::new(status_tx),
            state: Arc::new(watch::channel(StreamingState::default()).0),
            generation: 1,
            current: Arc::new(AtomicU64::new(1)),
            control: Arc::new(ObsControl::default()),
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
    // Faults for both fakes go through the fake Twitch's control port.
    control(
        f.port,
        &["fault", "ws-connect obs", "--close", "--for", "4s"],
    )
    .await;
    tokio::time::timeout(
        Duration::from_secs(5),
        status.wait_for(|s| s.state != ObsState::Connected),
    )
    .await
    .unwrap()
    .unwrap();
    tokio::time::timeout(
        Duration::from_secs(20),
        status.wait_for(|s| s.state == ObsState::Connected),
    )
    .await
    .expect("connected again after the fault")
    .unwrap();

    let report = control(f.port, &["stats"]).await;
    let connects = report["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["key"] == "ws-connect obs")
        .unwrap()["total"]
        .as_u64()
        .unwrap();
    assert!(connects >= 3, "refused attempts are logged: {connects}");
    assert_eq!(
        warnings_for(&report, "reconnect_backoff", "ws-connect obs"),
        0,
        "{report}"
    );
}

fn obs_client(port: u16) -> (watch::Receiver<ObsStatus>, Arc<ObsControl>) {
    let (status_tx, status) = watch::channel(ObsStatus::CONNECTING);
    let control = Arc::new(ObsControl::default());
    tokio::spawn(
        Obs {
            host: "127.0.0.1".into(),
            port,
            secrets: Arc::new(InMemory::default()),
            status: Arc::new(status_tx),
            state: Arc::new(watch::channel(StreamingState::default()).0),
            generation: 1,
            current: Arc::new(AtomicU64::new(1)),
            control: control.clone(),
        }
        .run(),
    );
    (status, control)
}

#[tokio::test]
async fn slow_obs_requests_overlap_and_a_drop_still_gets_through() {
    let f = start().await;
    let (mut status, obs) = obs_client(f.obs_port);
    tokio::time::timeout(
        Duration::from_secs(10),
        status.wait_for(|s| s.state == ObsState::Connected),
    )
    .await
    .unwrap()
    .unwrap();
    control(
        f.port,
        &["fault", "GetStats", "--delay", "700ms", "--for", "1m"],
    )
    .await;
    let slow: Vec<_> = (0..6)
        .map(|_| {
            let obs = obs.clone();
            tokio::spawn(async move { obs.stats().await })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let dropped = Instant::now();
    control(f.port, &["obs", "drop"]).await;
    tokio::time::timeout(
        Duration::from_secs(5),
        status.wait_for(|s| s.state != ObsState::Connected),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        dropped.elapsed() < Duration::from_millis(600),
        "the drop did not wait for the slow replies"
    );
    for task in slow {
        let _ = task.await;
    }
    // The client gives up on its requests at the drop; the delayed replies are logged when they finish.
    tokio::time::sleep(Duration::from_millis(800)).await;
    let after = control(f.port, &["stats"]).await;
    let stats_row = after["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["key"] == "GetStats")
        .expect("GetStats logged")
        .clone();
    assert!(
        stats_row["max_in_flight"].as_u64().unwrap() >= 5,
        "{stats_row}"
    );
    assert_eq!(warnings_for(&after, "in_flight", "GetStats"), 1, "{after}");
}

#[tokio::test]
async fn closing_irc_drops_the_chat_connection_and_it_comes_back() {
    let f = start().await;
    let (up_tx, mut up) = watch::channel(false);
    tokio::spawn(
        integrations::twitch::irc::FirstChats {
            url: format!("ws://127.0.0.1:{}/irc", f.port),
            auth: integrations::twitch::auth::TwitchAuth::new(
                "fake-client".into(),
                &format!("http://127.0.0.1:{}/oauth2", f.port),
                integrations::tls::https_client(integrations::tls::client_config().unwrap()),
                Arc::new(InMemory::with(
                    integrations::twitch::auth::fake_stored_token(),
                )),
            ),
            tls: integrations::tls::client_config().unwrap(),
            deliver: Arc::new(|_| {}),
            connected: Arc::new(move |c| {
                let _ = up_tx.send(c);
            }),
        }
        .run(),
    );
    tokio::time::timeout(Duration::from_secs(10), up.wait_for(|u| *u))
        .await
        .unwrap()
        .unwrap();
    control(
        f.port,
        &["fault", "ws-connect irc", "--close", "--for", "1500ms"],
    )
    .await;
    tokio::time::timeout(Duration::from_secs(5), up.wait_for(|u| !*u))
        .await
        .expect("the open IRC connection was dropped")
        .unwrap();
    tokio::time::timeout(Duration::from_secs(15), up.wait_for(|u| *u))
        .await
        .expect("and it came back after the fault")
        .unwrap();
}
