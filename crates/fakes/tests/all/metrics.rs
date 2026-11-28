//! The request log and its warnings, with real clients and with requests made wrong on purpose.

use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::{Arc, atomic::AtomicU64},
    time::Duration,
};

use fakes::{
    control,
    metrics::Recorder,
    obs::FakeObs,
    twitch::{FakeTwitch, PRELOADED_SCOPES, PRELOADED_TOKEN},
};
use integrations::{
    obs::{Obs, ObsControl, ObsState, ObsStatus},
    secrets::InMemory,
    tls,
    twitch::{
        FEATURE_SCOPES, MVP_SCOPES,
        auth::{AuthStatus, TwitchAuth, fake_stored_token},
        helix::Helix,
    },
};
use serde_json::{Value, json};
use tokio::{net::TcpListener, sync::watch};
use toolkit_core::streaming::StreamingState;

struct Fakes {
    obs: FakeObs,
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
    let app = twitch.router().merge(control::router(obs.clone(), twitch));
    tokio::spawn(async move { axum::serve(listener, app).await });
    Fakes {
        obs,
        obs_port,
        port,
    }
}

/// The stats report over the control API.
async fn stats(port: u16, reset: bool) -> Value {
    let body = json!({ "reset": reset });
    tokio::task::spawn_blocking(move || {
        serde_json::from_str(&control::call(port, "/_control/stats", &body).unwrap()).unwrap()
    })
    .await
    .unwrap()
}

/// A raw HTTP request with exactly these headers.
async fn raw(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> u16 {
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{}Content-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        headers
            .iter()
            .map(|(k, v)| format!("{k}: {v}\r\n"))
            .collect::<String>(),
        body.len()
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

const GOOD: &[(&str, &str)] = &[
    ("Client-Id", "fake-client"),
    ("Authorization", "Bearer fake-access"),
];

fn row<'a>(report: &'a Value, key: &str) -> &'a Value {
    report["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["key"] == key)
        .unwrap_or_else(|| panic!("{key} in {report}"))
}

fn warnings(report: &Value) -> Vec<String> {
    report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| {
            format!(
                "{} {}",
                w["kind"].as_str().unwrap(),
                w["key"].as_str().unwrap()
            )
        })
        .collect()
}

fn obs_client(
    port: u16,
) -> (
    watch::Receiver<ObsStatus>,
    watch::Receiver<StreamingState>,
    Arc<ObsControl>,
) {
    let (status_tx, status) = watch::channel(ObsStatus::CONNECTING);
    let (state_tx, state) = watch::channel(StreamingState::default());
    let control = Arc::new(ObsControl::default());
    tokio::spawn(
        Obs {
            host: "127.0.0.1".into(),
            port,
            secrets: Arc::new(InMemory::default()),
            status: Arc::new(status_tx),
            state: Arc::new(state_tx),
            generation: 1,
            current: Arc::new(AtomicU64::new(1)),
            control: control.clone(),
        }
        .run(),
    );
    (status, state, control)
}

#[test]
fn the_preloaded_token_is_the_one_the_dev_build_uses() {
    let stored: Value = serde_json::from_str(&fake_stored_token()).unwrap();
    assert_eq!(stored["access_token"], PRELOADED_TOKEN);
    let every: Vec<&str> = MVP_SCOPES.iter().chain(FEATURE_SCOPES).copied().collect();
    assert_eq!(stored["scopes"], serde_json::json!(every));
    assert_eq!(PRELOADED_SCOPES, every.as_slice());
}

#[tokio::test]
async fn counts_and_gaps_match_the_calls_made_and_reset_starts_over() {
    let f = start().await;
    let http = tls::https_client(tls::client_config().unwrap());
    let auth = TwitchAuth::new(
        "fake-client".into(),
        &format!("http://127.0.0.1:{}/oauth2", f.port),
        http.clone(),
        Arc::new(InMemory::with(fake_stored_token())),
    );
    let helix = Helix::new(&format!("http://127.0.0.1:{}/helix", f.port), auth, http);
    for _ in 0..3 {
        helix.get_json("/users").await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let (mut status, mut state, obs) = obs_client(f.obs_port);
    tokio::time::timeout(
        Duration::from_secs(10),
        status.wait_for(|s| s.state == ObsState::Connected),
    )
    .await
    .unwrap()
    .unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        state.wait_for(|s| !s.scenes.is_empty()),
    )
    .await
    .unwrap()
    .unwrap();
    obs.switch_scene("Live").await.unwrap();
    obs.switch_scene("Break").await.unwrap();

    let report = stats(f.port, true).await;
    let users = row(&report, "GET /helix/users");
    assert_eq!(users["total"], 3);
    let avg = users["avg_gap_ms"].as_f64().unwrap();
    assert!((280.0..450.0).contains(&avg), "avg gap {avg} ms");
    assert!(users["min_gap_ms"].as_f64().unwrap() >= 280.0);
    assert_eq!(row(&report, "SetCurrentProgramScene")["total"], 2);
    assert_eq!(row(&report, "ws-connect obs")["total"], 1);
    assert_eq!(report["helix"]["total"], 3);

    let after = stats(f.port, false).await;
    assert!(after["calls"].as_array().unwrap().is_empty(), "{after}");
}

#[tokio::test]
async fn each_anomaly_made_on_purpose_is_flagged() {
    let f = start().await;
    // An EventSub subscription, so the fake sends the follow below.
    let subscription = r#"{"type":"channel.follow","version":"2","condition":{},"transport":{"method":"websocket"}}"#;
    assert_eq!(
        raw(
            f.port,
            "POST",
            "/helix/eventsub/subscriptions",
            GOOD,
            subscription
        )
        .await,
        202
    );

    // Duplicates and a loop: the same request six times, 100 ms apart.
    for _ in 0..6 {
        raw(
            f.port,
            "GET",
            "/helix/streams?user_id=1000&type=live",
            GOOD,
            "",
        )
        .await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // Missing headers and an unknown token.
    raw(
        f.port,
        "GET",
        "/helix/users",
        &[("Authorization", "Bearer fake-access")],
        "",
    )
    .await;
    raw(
        f.port,
        "GET",
        "/helix/chat/badges/global",
        &[("Client-Id", "x"), ("Authorization", "Bearer nope")],
        "",
    )
    .await;
    raw(f.port, "GET", "/oauth2/validate", &[], "").await;
    // Amplification: a dozen different requests right after one event.
    tokio::task::spawn_blocking({
        let port = f.port;
        move || control::call(port, "/_control/send", &json!({"kind": "follow"})).unwrap()
    })
    .await
    .unwrap();
    // All at once, so they land within the reaction window even where a round trip is slow.
    let port = f.port;
    let burst: Vec<_> = (0..12)
        .map(|i| {
            tokio::spawn(async move {
                raw(port, "GET", &format!("/helix/users?id={i}"), GOOD, "").await
            })
        })
        .collect();
    for request in burst {
        request.await.unwrap();
    }
    // Reconnecting without backoff: four EventSub connections 100 ms apart.
    for _ in 0..4 {
        let (socket, _) =
            tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}/eventsub", f.port))
                .await
                .unwrap();
        drop(socket);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let found = warnings(&stats(f.port, false).await);
    for expected in [
        "duplicate GET /helix/streams",
        "tight_loop GET /helix/streams",
        "headers GET /helix/users",
        "headers GET /helix/chat/badges/global",
        "headers GET /oauth2/validate",
        "amplification channel.follow",
        "reconnect_backoff ws-connect eventsub",
    ] {
        assert!(
            found.iter().any(|w| w == expected),
            "{expected} in {found:#?}"
        );
    }
}

#[tokio::test]
async fn real_clients_raise_no_warnings() {
    let f = start().await;
    f.obs.add_inputs(
        &["Mic 2", "Music 2", "Game 2", "Webcam 2", "Overlay", "Clock"],
        "wasapi_input_capture",
    );
    let (mut status, mut state, _obs) = obs_client(f.obs_port);
    tokio::time::timeout(
        Duration::from_secs(10),
        status.wait_for(|s| s.state == ObsState::Connected),
    )
    .await
    .unwrap()
    .unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        state.wait_for(|s| s.inputs.len() >= 8),
    )
    .await
    .unwrap()
    .unwrap();

    let http = tls::https_client(tls::client_config().unwrap());
    let auth = TwitchAuth::new(
        "fake-client".into(),
        &format!("http://127.0.0.1:{}/oauth2", f.port),
        http,
        Arc::new(InMemory::default()),
    );
    auth.start_login(MVP_SCOPES).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !matches!(auth.status(), AuthStatus::SignedIn { .. }) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();

    let report = stats(f.port, false).await;
    assert_eq!(row(&report, "GET /oauth2/validate")["total"], 1);
    assert!(warnings(&report).is_empty(), "{:#?}", report["warnings"]);
}
