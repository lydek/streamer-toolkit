//! The real OBS client (`integrations::obs`) against the fake OBS.

use std::{
    sync::{Arc, Mutex, atomic::AtomicU64},
    time::Duration,
};

use fakes::obs::FakeObs;
use integrations::{
    obs::{Obs, ObsControl, ObsState, ObsStatus},
    secrets::{SecretStore, SecretStoreError},
};
use tokio::{net::TcpListener, sync::watch};
use toolkit_core::streaming::StreamingState;

#[derive(Default)]
struct Password(Mutex<Option<String>>);

impl SecretStore for Password {
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

struct Client {
    status: watch::Receiver<ObsStatus>,
    state: watch::Receiver<StreamingState>,
    control: Arc<ObsControl>,
}

async fn fake(password: Option<&str>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(FakeObs::new(password.map(str::to_owned)).serve(listener));
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
            secrets: Arc::new(Password(Mutex::new(password.map(str::to_owned)))),
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
    until(&mut c.state, "the scenes", |s| s.scenes.len() == 3).await;
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
    until(&mut c.state, "the scenes", |s| s.scenes.len() == 3).await;

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
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].name, "Chat");
    assert_eq!(sources[0].url, "http://127.0.0.1:47811/overlay/chat/");
}

#[tokio::test]
async fn studio_mode_previews_and_transitions() {
    let mut c = connect(fake(None).await, None);
    until(&mut c.state, "the scenes", |s| s.scenes.len() == 3).await;
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
