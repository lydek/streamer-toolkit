//! A fake obs-websocket 5 server with scenes, scene items, inputs, studio mode and transitions kept in memory.
//! It answers the requests `crates/integrations/src/obs.rs` sends (the `obws` 0.15 client) and sends the events a
//! real OBS would for each change. Anything else gets `UnknownRequestType`, so a request the fake lacks shows up.

use std::sync::{Arc, Mutex};

use axum::{
    Router,
    extract::{State, WebSocketUpgrade, ws},
    response::Response,
    routing::any,
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{net::TcpListener, sync::broadcast};

/// obs-websocket close code for a missing or wrong password.
const AUTHENTICATION_FAILED: u16 = 4009;
const CHALLENGE: &str = "fake-challenge";
const SALT: &str = "fake-salt";

// obs-websocket request status codes.
const SUCCESS: u16 = 100;
const UNKNOWN_REQUEST_TYPE: u16 = 204;
const MISSING_REQUEST_FIELD: u16 = 300;
const STUDIO_MODE_NOT_ACTIVE: u16 = 506;
const RESOURCE_NOT_FOUND: u16 = 600;
const RESOURCE_ALREADY_EXISTS: u16 = 601;
const INVALID_RESOURCE_STATE: u16 = 604;

type Failure = (u16, String);
type Event = (&'static str, Value);

#[derive(Clone)]
struct Input {
    name: String,
    uuid: String,
    kind: String,
    settings: Value,
    /// Inputs without audio answer mute and volume requests with `InvalidResourceState`, as in OBS.
    audio: bool,
    volume_mul: f64,
    muted: bool,
    monitor: &'static str,
}

#[derive(Clone)]
struct Item {
    id: u64,
    /// An input's or a scene's name.
    source: String,
    enabled: bool,
    locked: bool,
}

#[derive(Clone)]
struct Scene {
    name: String,
    uuid: String,
    group: bool,
    /// Bottom first: index 0 is the bottom item, as OBS counts.
    items: Vec<Item>,
}

struct World {
    /// In the order OBS shows them, top first.
    scenes: Vec<Scene>,
    inputs: Vec<Input>,
    program: String,
    preview: Option<String>,
    studio_mode: bool,
    /// `(name, uuid, kind)`.
    transitions: Vec<(String, String, String)>,
    transition: usize,
    streaming: bool,
    recording: bool,
    next_id: u64,
    next_uuid: u64,
}

fn uuid(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}

fn db(mul: f64) -> f64 {
    if mul <= 0.0 {
        -100.0
    } else {
        20.0 * mul.log10()
    }
}

impl World {
    /// Three scenes with a camera, a chat browser source, a logo, a microphone and desktop audio.
    fn sample() -> Self {
        let mut w = World {
            scenes: Vec::new(),
            inputs: Vec::new(),
            program: String::new(),
            preview: None,
            studio_mode: false,
            transitions: Vec::new(),
            transition: 1,
            streaming: false,
            recording: false,
            next_id: 1,
            next_uuid: 1,
        };
        for (name, kind, audio, settings) in [
            ("Mic", "wasapi_input_capture", true, json!({})),
            ("Desktop Audio", "wasapi_output_capture", true, json!({})),
            ("Camera", "dshow_input", false, json!({})),
            ("Logo", "image_source", false, json!({})),
            (
                "Chat",
                "browser_source",
                false,
                json!({"url": "http://127.0.0.1:47811/overlay/chat/", "width": 400, "height": 600}),
            ),
        ] {
            let uuid = w.uuid();
            w.inputs.push(Input {
                name: name.into(),
                uuid,
                kind: kind.into(),
                settings,
                audio,
                volume_mul: 1.0,
                muted: false,
                monitor: "OBS_MONITORING_TYPE_NONE",
            });
        }
        for (name, sources) in [
            ("Starting", &["Logo"][..]),
            ("Live", &["Camera", "Chat", "Logo"][..]),
            ("Break", &["Logo"][..]),
        ] {
            let uuid = w.uuid();
            let items = sources.iter().map(|s| w.item(s)).collect();
            w.scenes.push(Scene {
                name: name.into(),
                uuid,
                group: false,
                items,
            });
        }
        w.program = "Starting".into();
        for (name, kind) in [("Cut", "cut_transition"), ("Fade", "fade_transition")] {
            let uuid = w.uuid();
            w.transitions.push((name.into(), uuid, kind.into()));
        }
        w
    }

    fn uuid(&mut self) -> String {
        self.next_uuid += 1;
        uuid(self.next_uuid)
    }

    fn item(&mut self, source: &str) -> Item {
        self.next_id += 1;
        Item {
            id: self.next_id,
            source: source.into(),
            enabled: true,
            locked: false,
        }
    }

    fn scene_index(&self, data: &Value) -> Result<usize, Failure> {
        let by_name = data["sceneName"].as_str();
        let by_uuid = data["sceneUuid"].as_str();
        if by_name.is_none() && by_uuid.is_none() {
            return Err((MISSING_REQUEST_FIELD, "sceneName or sceneUuid".into()));
        }
        self.scenes
            .iter()
            .position(|s| Some(s.name.as_str()) == by_name || Some(s.uuid.as_str()) == by_uuid)
            .ok_or((RESOURCE_NOT_FOUND, "no such scene".into()))
    }

    fn input_index(&self, data: &Value) -> Result<usize, Failure> {
        let by_name = data["inputName"].as_str();
        let by_uuid = data["inputUuid"].as_str();
        if by_name.is_none() && by_uuid.is_none() {
            return Err((MISSING_REQUEST_FIELD, "inputName or inputUuid".into()));
        }
        self.inputs
            .iter()
            .position(|i| Some(i.name.as_str()) == by_name || Some(i.uuid.as_str()) == by_uuid)
            .ok_or((RESOURCE_NOT_FOUND, "no such input".into()))
    }

    fn audio_input(&self, data: &Value) -> Result<usize, Failure> {
        let i = self.input_index(data)?;
        if !self.inputs[i].audio {
            return Err((
                INVALID_RESOURCE_STATE,
                "The specified input does not support audio.".into(),
            ));
        }
        Ok(i)
    }

    fn item_index(&self, scene: usize, data: &Value) -> Result<usize, Failure> {
        let id = data["sceneItemId"]
            .as_u64()
            .ok_or((MISSING_REQUEST_FIELD, "sceneItemId".into()))?;
        self.scenes[scene]
            .items
            .iter()
            .position(|i| i.id == id)
            .ok_or((RESOURCE_NOT_FOUND, "no such scene item".into()))
    }

    fn scene_id(&self, scene: usize) -> Value {
        let s = &self.scenes[scene];
        json!({"sceneName": s.name, "sceneUuid": s.uuid})
    }

    fn scene_by_name(&self, name: &str) -> Option<usize> {
        self.scenes.iter().position(|s| s.name == name)
    }

    /// `(uuid, sourceType, inputKind, isGroup)` of a source by name.
    fn source(&self, name: &str) -> Option<(String, &'static str, Value, Value)> {
        if let Some(i) = self.inputs.iter().find(|i| i.name == name) {
            return Some((
                i.uuid.clone(),
                "OBS_SOURCE_TYPE_INPUT",
                json!(i.kind),
                Value::Null,
            ));
        }
        self.scenes.iter().find(|s| s.name == name).map(|s| {
            (
                s.uuid.clone(),
                "OBS_SOURCE_TYPE_SCENE",
                Value::Null,
                json!(s.group),
            )
        })
    }

    fn items_json(&self, scene: usize) -> Value {
        let items: Vec<Value> = self.scenes[scene]
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let (uuid, kind, input_kind, group) = self.source(&item.source).unwrap_or_default();
                json!({"sceneItemId": item.id, "sceneItemIndex": index, "sourceName": item.source,
                       "sourceUuid": uuid, "sourceType": kind, "inputKind": input_kind, "isGroup": group,
                       "sceneItemEnabled": item.enabled, "sceneItemLocked": item.locked})
            })
            .collect();
        json!({ "sceneItems": items })
    }

    fn scene_changed(&self, kind: &'static str, name: &str) -> Event {
        let i = self.scene_by_name(name).unwrap_or_default();
        (kind, self.scene_id(i))
    }

    /// Answers one request and lists the events it causes.
    fn handle(&mut self, request: &str, data: &Value) -> (Result<Value, Failure>, Vec<Event>) {
        let mut events = Vec::new();
        let result = self.answer(request, data, &mut events);
        (result, events)
    }

    fn answer(
        &mut self,
        request: &str,
        data: &Value,
        events: &mut Vec<Event>,
    ) -> Result<Value, Failure> {
        Ok(match request {
            "GetVersion" => {
                json!({"obsVersion": "31.0.0", "obsWebSocketVersion": "5.5.0", "rpcVersion": 1,
                "availableRequests": [], "supportedImageFormats": [], "platform": "windows",
                "platformDescription": "fake-services"})
            }
            "GetStats" => {
                json!({"cpuUsage": 2.5, "memoryUsage": 300.0, "availableDiskSpace": 100000.0,
                "activeFps": 60.0, "averageFrameRenderTime": 2.0, "renderSkippedFrames": 0,
                "renderTotalFrames": 0, "outputSkippedFrames": 0, "outputTotalFrames": 0,
                "webSocketSessionIncomingMessages": 0, "webSocketSessionOutgoingMessages": 0})
            }
            "GetStreamStatus" => {
                json!({"outputActive": self.streaming, "outputReconnecting": false,
                "outputTimecode": "00:00:00.000", "outputDuration": 0, "outputCongestion": 0.0,
                "outputBytes": 0, "outputSkippedFrames": 0, "outputTotalFrames": 0})
            }
            "GetRecordStatus" => json!({"outputActive": self.recording, "outputPaused": false,
                "outputTimecode": "00:00:00.000", "outputDuration": 0, "outputBytes": 0}),
            "GetSceneList" => {
                let program = self.scene_by_name(&self.program).unwrap_or_default();
                let preview = self.preview.as_deref().and_then(|p| self.scene_by_name(p));
                // OBS lists scenes bottom first; groups are no scenes here.
                let scenes: Vec<Value> = self
                    .scenes
                    .iter()
                    .filter(|s| !s.group)
                    .rev()
                    .enumerate()
                    .map(|(index, s)| json!({"sceneName": s.name, "sceneUuid": s.uuid, "sceneIndex": index}))
                    .collect();
                json!({"currentProgramSceneName": self.scenes[program].name,
                       "currentProgramSceneUuid": self.scenes[program].uuid,
                       "currentPreviewSceneName": preview.map(|p| &self.scenes[p].name),
                       "currentPreviewSceneUuid": preview.map(|p| &self.scenes[p].uuid),
                       "scenes": scenes})
            }
            "GetSceneItemList" => {
                let scene = self.scene_index(data)?;
                if self.scenes[scene].group {
                    return Err((RESOURCE_NOT_FOUND, "a group is not a scene".into()));
                }
                self.items_json(scene)
            }
            "GetGroupSceneItemList" => {
                let scene = self.scene_index(data)?;
                if !self.scenes[scene].group {
                    return Err((RESOURCE_NOT_FOUND, "not a group".into()));
                }
                self.items_json(scene)
            }
            "GetSceneItemEnabled" => {
                let scene = self.scene_index(data)?;
                let item = self.item_index(scene, data)?;
                json!({"sceneItemEnabled": self.scenes[scene].items[item].enabled})
            }
            "GetSceneItemLocked" => {
                let scene = self.scene_index(data)?;
                let item = self.item_index(scene, data)?;
                json!({"sceneItemLocked": self.scenes[scene].items[item].locked})
            }
            "GetInputList" => {
                let kind = data["inputKind"].as_str();
                let inputs: Vec<Value> = self
                    .inputs
                    .iter()
                    .filter(|i| kind.is_none_or(|k| i.kind == k))
                    .map(|i| {
                        json!({"inputName": i.name, "inputUuid": i.uuid, "inputKind": i.kind,
                                    "unversionedInputKind": i.kind})
                    })
                    .collect();
                json!({ "inputs": inputs })
            }
            "GetInputSettings" => {
                let input = &self.inputs[self.input_index(data)?];
                json!({"inputKind": input.kind, "inputSettings": input.settings})
            }
            "GetSpecialInputs" => {
                json!({"desktop1": "Desktop Audio", "desktop2": null, "mic1": "Mic",
                "mic2": null, "mic3": null, "mic4": null})
            }
            "GetInputMute" => json!({"inputMuted": self.inputs[self.audio_input(data)?].muted}),
            "GetInputVolume" => {
                let mul = self.inputs[self.audio_input(data)?].volume_mul;
                json!({"inputVolumeMul": mul, "inputVolumeDb": db(mul)})
            }
            "GetInputAudioMonitorType" => {
                json!({"monitorType": self.inputs[self.audio_input(data)?].monitor})
            }
            "GetStudioModeEnabled" => json!({"studioModeEnabled": self.studio_mode}),
            "GetSceneTransitionList" => {
                let (name, uuid, kind) = &self.transitions[self.transition];
                let list: Vec<Value> = self
                    .transitions
                    .iter()
                    .map(|(n, u, k)| json!({"transitionName": n, "transitionUuid": u, "transitionKind": k,
                        "transitionFixed": k == "cut_transition", "transitionConfigurable": k != "cut_transition"}))
                    .collect();
                json!({"currentSceneTransitionName": name, "currentSceneTransitionUuid": uuid,
                       "currentSceneTransitionKind": kind, "transitions": list})
            }
            "SetCurrentProgramScene" => {
                let scene = self.scene_index(data)?;
                self.program = self.scenes[scene].name.clone();
                events.push(("CurrentProgramSceneChanged", self.scene_id(scene)));
                Value::Null
            }
            "SetCurrentPreviewScene" => {
                if !self.studio_mode {
                    return Err((STUDIO_MODE_NOT_ACTIVE, "studio mode is off".into()));
                }
                let scene = self.scene_index(data)?;
                self.preview = Some(self.scenes[scene].name.clone());
                events.push(("CurrentPreviewSceneChanged", self.scene_id(scene)));
                Value::Null
            }
            "SetStudioModeEnabled" => {
                let enabled = data["studioModeEnabled"]
                    .as_bool()
                    .ok_or((MISSING_REQUEST_FIELD, "studioModeEnabled".into()))?;
                if enabled != self.studio_mode {
                    self.studio_mode = enabled;
                    self.preview = enabled.then(|| self.program.clone());
                    events.push((
                        "StudioModeStateChanged",
                        json!({"studioModeEnabled": enabled}),
                    ));
                }
                Value::Null
            }
            "TriggerStudioModeTransition" => {
                let Some(preview) = self.preview.clone().filter(|_| self.studio_mode) else {
                    return Err((STUDIO_MODE_NOT_ACTIVE, "studio mode is off".into()));
                };
                let old = std::mem::replace(&mut self.program, preview);
                self.preview = Some(old.clone());
                events
                    .push(self.scene_changed("CurrentProgramSceneChanged", &self.program.clone()));
                events.push(self.scene_changed("CurrentPreviewSceneChanged", &old));
                Value::Null
            }
            "SetCurrentSceneTransition" => {
                let name = data["transitionName"]
                    .as_str()
                    .ok_or((MISSING_REQUEST_FIELD, "transitionName".into()))?;
                let t = self
                    .transitions
                    .iter()
                    .position(|(n, ..)| n == name)
                    .ok_or((RESOURCE_NOT_FOUND, "no such transition".into()))?;
                self.transition = t;
                let (name, uuid, _) = &self.transitions[t];
                events.push((
                    "CurrentSceneTransitionChanged",
                    json!({"transitionName": name, "transitionUuid": uuid}),
                ));
                Value::Null
            }
            "SetSceneItemEnabled" | "SetSceneItemLocked" => {
                let scene = self.scene_index(data)?;
                let item = self.item_index(scene, data)?;
                let (field, kind) = if request == "SetSceneItemEnabled" {
                    ("sceneItemEnabled", "SceneItemEnableStateChanged")
                } else {
                    ("sceneItemLocked", "SceneItemLockStateChanged")
                };
                let value = data[field]
                    .as_bool()
                    .ok_or((MISSING_REQUEST_FIELD, field.into()))?;
                let it = &mut self.scenes[scene].items[item];
                if request == "SetSceneItemEnabled" {
                    it.enabled = value;
                } else {
                    it.locked = value;
                }
                let mut event = self.scene_id(scene);
                event["sceneItemId"] = json!(self.scenes[scene].items[item].id);
                event[field] = json!(value);
                events.push((kind, event));
                Value::Null
            }
            "CreateSceneItem" => {
                let scene = self.scene_index(data)?;
                let source = match (data["sourceName"].as_str(), data["sourceUuid"].as_str()) {
                    (Some(name), _) => name.to_owned(),
                    (None, Some(uuid)) => self
                        .inputs
                        .iter()
                        .map(|i| (&i.name, &i.uuid))
                        .chain(self.scenes.iter().map(|s| (&s.name, &s.uuid)))
                        .find(|(_, u)| *u == uuid)
                        .map(|(n, _)| n.clone())
                        .ok_or((RESOURCE_NOT_FOUND, "no such source".into()))?,
                    _ => return Err((MISSING_REQUEST_FIELD, "sourceName or sourceUuid".into())),
                };
                if self.source(&source).is_none() || source == self.scenes[scene].name {
                    return Err((RESOURCE_NOT_FOUND, "no such source".into()));
                }
                let id = self.add_item(scene, &source, data["sceneItemEnabled"].as_bool(), events);
                json!({ "sceneItemId": id })
            }
            "RemoveSceneItem" => {
                let scene = self.scene_index(data)?;
                let item = self.item_index(scene, data)?;
                let removed = self.scenes[scene].items.remove(item);
                let (uuid, ..) = self.source(&removed.source).unwrap_or_default();
                let mut event = self.scene_id(scene);
                event["sourceName"] = json!(removed.source);
                event["sourceUuid"] = json!(uuid);
                event["sceneItemId"] = json!(removed.id);
                events.push(("SceneItemRemoved", event));
                Value::Null
            }
            "SetSceneItemIndex" => {
                let scene = self.scene_index(data)?;
                let item = self.item_index(scene, data)?;
                let to = data["sceneItemIndex"]
                    .as_u64()
                    .ok_or((MISSING_REQUEST_FIELD, "sceneItemIndex".into()))?
                    as usize;
                let items = &mut self.scenes[scene].items;
                let moved = items.remove(item);
                items.insert(to.min(items.len()), moved);
                let list: Vec<Value> = items
                    .iter()
                    .enumerate()
                    .map(|(index, i)| json!({"sceneItemId": i.id, "sceneItemIndex": index}))
                    .collect();
                let mut event = self.scene_id(scene);
                event["sceneItems"] = json!(list);
                events.push(("SceneItemListReindexed", event));
                Value::Null
            }
            "CreateInput" => {
                let scene = self.scene_index(data)?;
                let name = data["inputName"]
                    .as_str()
                    .ok_or((MISSING_REQUEST_FIELD, "inputName".into()))?;
                let kind = data["inputKind"]
                    .as_str()
                    .ok_or((MISSING_REQUEST_FIELD, "inputKind".into()))?;
                if self.source(name).is_some() {
                    return Err((
                        RESOURCE_ALREADY_EXISTS,
                        "a source with that name exists".into(),
                    ));
                }
                let uuid = self.uuid();
                let settings = data
                    .get("inputSettings")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                self.inputs.push(Input {
                    name: name.into(),
                    uuid: uuid.clone(),
                    kind: kind.into(),
                    settings: settings.clone(),
                    audio: kind.starts_with("wasapi_"),
                    volume_mul: 1.0,
                    muted: false,
                    monitor: "OBS_MONITORING_TYPE_NONE",
                });
                events.push((
                    "InputCreated",
                    json!({"inputName": name, "inputUuid": uuid, "inputKind": kind,
                           "unversionedInputKind": kind, "inputKindCaps": 0, "inputSettings": settings,
                           "defaultInputSettings": {}}),
                ));
                let id = self.add_item(scene, name, data["sceneItemEnabled"].as_bool(), events);
                json!({"inputUuid": uuid, "sceneItemId": id})
            }
            _ => {
                return Err((
                    UNKNOWN_REQUEST_TYPE,
                    format!("fake OBS does not answer {request}"),
                ));
            }
        })
    }

    /// A new item on top of the scene.
    fn add_item(
        &mut self,
        scene: usize,
        source: &str,
        enabled: Option<bool>,
        events: &mut Vec<Event>,
    ) -> u64 {
        let mut item = self.item(source);
        item.enabled = enabled.unwrap_or(true);
        let id = item.id;
        self.scenes[scene].items.push(item);
        let (uuid, ..) = self.source(source).unwrap_or_default();
        let mut event = self.scene_id(scene);
        event["sourceName"] = json!(source);
        event["sourceUuid"] = json!(uuid);
        event["sceneItemId"] = json!(id);
        event["sceneItemIndex"] = json!(self.scenes[scene].items.len() - 1);
        events.push(("SceneItemCreated", event));
        id
    }
}

/// The fake OBS: one shared world for every connection, so all clients see each change.
#[derive(Clone)]
pub struct FakeObs {
    world: Arc<Mutex<World>>,
    events: broadcast::Sender<Event>,
    password: Option<String>,
}

impl FakeObs {
    /// Without a password the fake asks for no authentication.
    pub fn new(password: Option<String>) -> Self {
        Self {
            world: Arc::new(Mutex::new(World::sample())),
            events: broadcast::channel(1024).0,
            password,
        }
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/", any(session))
            .with_state(self.clone())
    }

    /// Serves until the listener fails; bind it to `127.0.0.1` only.
    pub async fn serve(self, listener: TcpListener) -> std::io::Result<()> {
        axum::serve(listener, self.router()).await
    }

    /// Answers one request as a connection would, sending its events to every connection.
    fn request(&self, request: &str, data: &Value) -> Result<Value, Failure> {
        let (result, events) = self.world.lock().unwrap().handle(request, data);
        for event in events {
            // No connection listening is fine.
            let _ = self.events.send(event);
        }
        result
    }
}

/// The `authentication` string a client sends for `password` (obs-websocket 5 protocol).
fn auth_response(password: &str) -> String {
    let secret = BASE64.encode(Sha256::digest(format!("{password}{SALT}")));
    BASE64.encode(Sha256::digest(format!("{secret}{CHALLENGE}")))
}

async fn send(socket: &mut ws::WebSocket, value: Value) -> bool {
    socket
        .send(ws::Message::Text(value.to_string().into()))
        .await
        .is_ok()
}

async fn session(upgrade: WebSocketUpgrade, State(fake): State<FakeObs>) -> Response {
    upgrade.on_upgrade(move |mut socket| async move {
        let auth = fake
            .password
            .as_ref()
            .map(|_| json!({"challenge": CHALLENGE, "salt": SALT}));
        let hello = json!({"op": 0, "d": {"obsWebSocketVersion": "5.5.0", "rpcVersion": 1, "authentication": auth}});
        if !send(&mut socket, hello).await {
            return;
        }
        let Some(Ok(ws::Message::Text(identify))) = socket.recv().await else {
            return;
        };
        let identify: Value = serde_json::from_str(identify.as_str()).unwrap_or_default();
        if let Some(password) = &fake.password
            && identify["d"]["authentication"].as_str() != Some(auth_response(password).as_str())
        {
            let frame = ws::CloseFrame {
                code: AUTHENTICATION_FAILED,
                reason: "Authentication failed.".into(),
            };
            let _ = socket.send(ws::Message::Close(Some(frame))).await;
            return;
        }
        if !send(&mut socket, json!({"op": 2, "d": {"negotiatedRpcVersion": 1}})).await {
            return;
        }
        let mut events = fake.events.subscribe();
        loop {
            tokio::select! {
                message = socket.recv() => match message {
                    Some(Ok(ws::Message::Text(text))) => {
                        let message: Value = serde_json::from_str(text.as_str()).unwrap_or_default();
                        let reply = match message["op"].as_u64() {
                            // Reidentify.
                            Some(3) => json!({"op": 2, "d": {"negotiatedRpcVersion": 1}}),
                            Some(6) => reply(&fake, &message["d"]),
                            _ => continue,
                        };
                        if !send(&mut socket, reply).await {
                            return;
                        }
                    }
                    Some(Ok(_)) => {}
                    _ => return,
                },
                event = events.recv() => match event {
                    Ok((kind, data)) => {
                        let event = json!({"op": 5, "d": {"eventType": kind, "eventIntent": 1, "eventData": data}});
                        if !send(&mut socket, event).await {
                            return;
                        }
                    }
                    // A slow client misses some events, as with a real OBS under load.
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                },
            }
        }
    })
}

/// The `RequestResponse` (op 7) for a `Request` (op 6).
fn reply(fake: &FakeObs, request: &Value) -> Value {
    let request_type = request["requestType"].as_str().unwrap_or_default();
    let data = request.get("requestData").cloned().unwrap_or(Value::Null);
    let (status, response) = match fake.request(request_type, &data) {
        Ok(response) => (json!({"result": true, "code": SUCCESS}), response),
        Err((code, comment)) => (
            json!({"result": false, "code": code, "comment": comment}),
            Value::Null,
        ),
    };
    json!({"op": 7, "d": {"requestType": request_type, "requestId": request["requestId"],
                          "requestStatus": status, "responseData": response}})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> World {
        World::sample()
    }

    #[test]
    fn the_auth_response_matches_the_protocol_example_shape() {
        // Deterministic and base64 of a SHA-256 digest (44 characters).
        assert_eq!(auth_response("pw"), auth_response("pw"));
        assert_ne!(auth_response("pw"), auth_response("other"));
        assert_eq!(auth_response("pw").len(), 44);
    }

    #[test]
    fn scene_lists_are_bottom_first_and_items_keep_their_order() {
        let mut w = world();
        let list = w.handle("GetSceneList", &Value::Null).0.unwrap();
        let names: Vec<&str> = list["scenes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["sceneName"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["Break", "Live", "Starting"]);
        let items = w
            .handle("GetSceneItemList", &json!({"sceneName": "Live"}))
            .0
            .unwrap();
        assert_eq!(items["sceneItems"][0]["sourceName"], "Camera");
        assert_eq!(items["sceneItems"][1]["inputKind"], "browser_source");
    }

    #[test]
    fn inputs_without_audio_refuse_audio_requests() {
        let mut w = world();
        let (logo, _) = w.handle("GetInputMute", &json!({"inputName": "Logo"}));
        assert_eq!(logo.unwrap_err().0, INVALID_RESOURCE_STATE);
        let (mic, _) = w.handle("GetInputVolume", &json!({"inputName": "Mic"}));
        assert_eq!(mic.unwrap()["inputVolumeDb"], 0.0);
    }

    #[test]
    fn items_move_and_report_the_new_order() {
        let mut w = world();
        let camera = w.scenes[1].items[0].id;
        let (result, events) = w.handle(
            "SetSceneItemIndex",
            &json!({"sceneName": "Live", "sceneItemId": camera, "sceneItemIndex": 2}),
        );
        result.unwrap();
        assert_eq!(w.scenes[1].items[2].source, "Camera");
        assert_eq!(events[0].0, "SceneItemListReindexed");
        assert_eq!(events[0].1["sceneItems"][2]["sceneItemId"], camera);
    }

    #[test]
    fn studio_mode_transitions_swap_program_and_preview() {
        let mut w = world();
        assert_eq!(
            w.handle("SetCurrentPreviewScene", &json!({"sceneName": "Live"}))
                .0
                .unwrap_err()
                .0,
            STUDIO_MODE_NOT_ACTIVE
        );
        w.handle("SetStudioModeEnabled", &json!({"studioModeEnabled": true}))
            .0
            .unwrap();
        w.handle("SetCurrentPreviewScene", &json!({"sceneName": "Live"}))
            .0
            .unwrap();
        let (result, events) = w.handle("TriggerStudioModeTransition", &Value::Null);
        result.unwrap();
        assert_eq!(
            (w.program.as_str(), w.preview.as_deref()),
            ("Live", Some("Starting"))
        );
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn unknown_requests_and_missing_resources_fail_like_obs() {
        let mut w = world();
        assert_eq!(
            w.handle("StartStream", &Value::Null).0.unwrap_err().0,
            UNKNOWN_REQUEST_TYPE
        );
        assert_eq!(
            w.handle("SetCurrentProgramScene", &json!({"sceneName": "Nope"}))
                .0
                .unwrap_err()
                .0,
            RESOURCE_NOT_FOUND
        );
        assert_eq!(
            w.handle(
                "CreateInput",
                &json!({"sceneName": "Live", "inputName": "Logo", "inputKind": "x"})
            )
            .0
            .unwrap_err()
            .0,
            RESOURCE_ALREADY_EXISTS
        );
    }
}
