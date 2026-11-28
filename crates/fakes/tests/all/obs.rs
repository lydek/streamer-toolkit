//! The real OBS client (`integrations::obs`) against the fake OBS.

use std::{
    sync::{Arc, atomic::AtomicU64},
    time::Duration,
};

use fakes::obs::{FakeObs, Stats};
use integrations::{
    obs::{Obs, ObsControl, ObsState, ObsStatus},
    secrets::InMemory,
};
use tokio::{net::TcpListener, sync::watch};
use toolkit_core::streaming::{ItemSize, Monitor, StreamingState, TransformChange};

struct Client {
    status: watch::Receiver<ObsStatus>,
    state: watch::Receiver<StreamingState>,
    control: Arc<ObsControl>,
}

async fn fake(password: Option<&str>) -> u16 {
    start(FakeObs::new(password.map(str::to_owned))).await
}

async fn start(fake: FakeObs) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(fake.serve(listener));
    port
}

fn connect(port: u16, password: Option<&str>) -> Client {
    let (status_tx, status) = watch::channel(ObsStatus::CONNECTING);
    let (state_tx, state) = watch::channel(StreamingState::default());
    let control = Arc::new(ObsControl::default());
    tokio::spawn(
        Obs {
            host: "127.0.0.1".into(),
            port,
            secrets: Arc::new(
                password
                    .map(|p| InMemory::with(p.into()))
                    .unwrap_or_default(),
            ),
            status: Arc::new(status_tx),
            state: Arc::new(state_tx),
            generation: 1,
            current: Arc::new(AtomicU64::new(1)),
            control: control.clone(),
        }
        .run(),
    );
    Client {
        status,
        state,
        control,
    }
}

async fn until<T>(rx: &mut watch::Receiver<T>, what: &str, done: impl FnMut(&T) -> bool) {
    tokio::time::timeout(Duration::from_secs(10), rx.wait_for(done))
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
        .unwrap();
}

fn items(state: &StreamingState, scene: &str) -> Vec<String> {
    state
        .scenes
        .iter()
        .find(|s| s.name == scene)
        .map(|s| s.items.iter().map(|i| i.source.clone()).collect())
        .unwrap_or_default()
}

#[tokio::test]
async fn connects_without_a_password_and_reads_scenes_and_sources() {
    let mut c = connect(fake(None).await, None);
    until(&mut c.status, "connected", |s| {
        s.state == ObsState::Connected
    })
    .await;
    until(&mut c.state, "the scenes", |s| s.scenes.len() == 12).await;
    let state = c.state.borrow().clone();
    assert_eq!(state.scene.as_deref(), Some("Starting"));
    // Front to back: the top item first.
    assert_eq!(items(&state, "Live"), ["Logo", "Chat", "Camera"]);
    let mic = state
        .inputs
        .iter()
        .find(|i| i.name == "Mic")
        .expect("the microphone");
    assert!(!mic.muted);
    assert!(
        state.inputs.iter().all(|i| i.name != "Logo"),
        "inputs without audio are no mixer inputs"
    );
}

#[tokio::test]
async fn switching_scenes_and_editing_sources_come_back_as_events() {
    let mut c = connect(fake(None).await, None);
    until(&mut c.state, "the scenes", |s| s.scenes.len() == 12).await;

    c.control.switch_scene("Live").await.unwrap();
    until(&mut c.state, "the program scene", |s| {
        s.scene.as_deref() == Some("Live")
    })
    .await;

    c.control.add_source("Break", "Camera").await.unwrap();
    until(&mut c.state, "the new item", |s| {
        items(s, "Break") == ["Camera", "Logo"]
    })
    .await;

    let camera = c
        .state
        .borrow()
        .scenes
        .iter()
        .find(|s| s.name == "Break")
        .unwrap()
        .items[0]
        .id;
    c.control
        .set_item_enabled("Break", camera, false)
        .await
        .unwrap();
    until(&mut c.state, "the hidden item", |s| {
        s.scenes
            .iter()
            .any(|sc| sc.name == "Break" && sc.items.iter().any(|i| i.id == camera && !i.enabled))
    })
    .await;

    c.control.remove_item("Break", camera).await.unwrap();
    until(&mut c.state, "the removed item", |s| {
        items(s, "Break") == ["Logo"]
    })
    .await;

    c.control
        .create_browser_source(
            "Live",
            "Alerts",
            "http://127.0.0.1:47811/overlay/alerts/",
            (800, 600),
        )
        .await
        .unwrap();
    until(&mut c.state, "the browser source", |s| {
        items(s, "Live").first().map(String::as_str) == Some("Alerts")
    })
    .await;
}

#[tokio::test]
async fn browser_sources_list_their_urls() {
    let mut c = connect(fake(None).await, None);
    until(&mut c.status, "connected", |s| {
        s.state == ObsState::Connected
    })
    .await;
    let sources = c.control.browser_sources().await.unwrap();
    assert_eq!(sources.len(), 11);
    let chat = sources.iter().find(|s| s.name == "Chat").unwrap();
    assert_eq!(chat.url, "http://127.0.0.1:47811/overlay/chat/");
}

#[tokio::test]
async fn studio_mode_previews_and_transitions() {
    let mut c = connect(fake(None).await, None);
    until(&mut c.state, "the scenes", |s| s.scenes.len() == 12).await;
    c.control.set_studio_mode(true).await.unwrap();
    until(&mut c.state, "studio mode", |s| s.studio_mode).await;
    c.control.set_preview("Break").await.unwrap();
    until(&mut c.state, "the preview", |s| {
        s.preview.as_deref() == Some("Break")
    })
    .await;
    c.control.transition(None).await.unwrap();
    until(&mut c.state, "the transition", |s| {
        s.scene.as_deref() == Some("Break") && s.preview.as_deref() == Some("Starting")
    })
    .await;
}

#[tokio::test]
async fn a_password_must_match() {
    let port = fake(Some("secret")).await;
    let mut right = connect(port, Some("secret"));
    until(&mut right.status, "connected", |s| {
        s.state == ObsState::Connected
    })
    .await;
    let mut wrong = connect(port, Some("nope"));
    until(&mut wrong.status, "refused", |s| s.state == ObsState::Error).await;
}

fn input(state: &StreamingState, name: &str) -> Option<toolkit_core::streaming::Input> {
    state.inputs.iter().find(|i| i.name == name).cloned()
}

#[tokio::test]
async fn volume_mute_and_monitoring_are_kept_and_reported() {
    let mut c = connect(fake(None).await, None);
    until(&mut c.state, "the mixer", |s| input(s, "Mic").is_some()).await;
    c.control.set_input_volume("Mic", -6.0).await.unwrap();
    until(&mut c.state, "the volume", |s| {
        input(s, "Mic").is_some_and(|m| (m.volume_db + 6.0).abs() < 0.01)
    })
    .await;
    c.control.set_input_mute("Mic", true).await.unwrap();
    until(&mut c.state, "the mute", |s| {
        input(s, "Mic").is_some_and(|m| m.muted)
    })
    .await;
    c.control
        .set_input_monitor("Mic", Monitor::MonitorOnly)
        .await
        .unwrap();
    until(&mut c.state, "the monitoring", |s| {
        input(s, "Mic").is_some_and(|m| m.monitor == Monitor::MonitorOnly)
    })
    .await;
    assert!(
        c.control.set_input_mute("Logo", true).await.is_err(),
        "no audio"
    );
}

#[tokio::test]
async fn sizes_transforms_and_settings_read_back_as_written() {
    let mut c = connect(fake(None).await, None);
    until(&mut c.state, "the scenes", |s| s.scenes.len() == 12).await;
    let camera = c
        .state
        .borrow()
        .scenes
        .iter()
        .find(|s| s.name == "Live")
        .and_then(|s| s.items.iter().find(|i| i.source == "Camera"))
        .unwrap()
        .id;

    c.control
        .set_item_size(
            "Live",
            camera,
            ItemSize {
                width: 960.0,
                height: 540.0,
            },
        )
        .await
        .unwrap();
    let (canvas, items) = c.control.placements("Live").await.unwrap();
    assert_eq!((canvas.width, canvas.height), (1920, 1080));
    let (_, _, size) = items.iter().find(|(id, ..)| *id == camera).unwrap();
    assert_eq!((size.width.round(), size.height.round()), (960.0, 540.0));

    let change = TransformChange {
        rotation: Some(90.0),
        ..Default::default()
    };
    c.control
        .set_item_transform("Live", camera, change)
        .await
        .unwrap();
    assert_eq!(
        c.control
            .item_transform("Live", camera)
            .await
            .unwrap()
            .rotation,
        90.0
    );

    let mut width = serde_json::Map::new();
    width.insert("width".into(), 500.into());
    c.control.set_input_settings("Chat", width).await.unwrap();
    let (kind, settings) = c.control.input_settings("Chat").await.unwrap();
    assert_eq!(kind, "browser_source");
    assert_eq!(settings["width"], 500);
    assert_eq!(
        settings["url"], "http://127.0.0.1:47811/overlay/chat/",
        "merged, not replaced"
    );
    assert_eq!(settings["css"], "", "defaults fill the rest");

    let cameras = c
        .control
        .input_list_items("Camera", "video_device_id")
        .await
        .unwrap();
    // The busy (disabled) camera is left out by the client.
    assert_eq!(cameras.len(), 1);
    assert_eq!(cameras[0].0, "Fake Camera");
    c.control
        .press_input_button("Chat", "refreshnocache")
        .await
        .unwrap();
}

#[tokio::test]
async fn stats_follow_the_set_drop_share_and_bitrate() {
    let obs = FakeObs::new(None);
    let mut c = connect(start(obs.clone()).await, None);
    until(&mut c.state, "the scenes", |s| s.scenes.len() == 12).await;
    obs.set_output(Some(true), None);
    until(&mut c.state, "streaming", |s| s.streaming).await;
    obs.set_stats(Stats {
        cpu_percent: 40.0,
        dropped_percent: 8.0,
        bitrate_kbps: 4000,
    });
    let before = c.control.stats().await.unwrap();
    let at = std::time::Instant::now();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let after = c.control.stats().await.unwrap();
    let secs = at.elapsed().as_secs_f64();
    assert_eq!(after.cpu_percent, 40.0);
    let skipped = (after.render_skipped - before.render_skipped) as f64;
    let total = (after.render_total - before.render_total) as f64;
    assert!(
        (skipped / total * 100.0 - 8.0).abs() < 1.5,
        "{skipped}/{total}"
    );
    let kbps = (after.stream_bytes - before.stream_bytes) as f64 * 8.0 / 1000.0 / secs;
    assert!((kbps - 4000.0).abs() < 400.0, "{kbps} kbps");
}

#[tokio::test]
async fn audio_levels_arrive_like_obs_sends_them() {
    let mut c = connect(fake(None).await, None);
    until(&mut c.status, "connected", |s| {
        s.state == ObsState::Connected
    })
    .await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !c.control.meters().iter().any(|m| m.name == "Mic") {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("meters for the microphone");
}
