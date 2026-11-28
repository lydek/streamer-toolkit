//! A fake obs-websocket 5 server with scenes, scene items, inputs, studio mode and transitions kept in memory.
//! It answers the requests `crates/integrations/src/obs.rs` sends (the `obws` 0.15 client) and sends the events a
//! real OBS would for each change. Anything else gets `UnknownRequestType`, so a request the fake lacks shows up.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

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

use crate::metrics::{self, Call, Recorder};

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
    /// Fields set with `SetSceneItemTransform`; the rest come from the source's size.
    transform: Value,
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
    stats: Stats,
    counters: Counters,
    meter_interval: Duration,
    /// Extra inputs in the audio meters only, for load runs.
    meter_extra: usize,
}

/// What the fake OBS looks like now, for the control panel.
#[derive(Debug, Clone, PartialEq)]
pub struct ObsSnapshot {
    /// Connections open right now.
    pub open: u32,
    /// In the order OBS shows them, top first; each scene's items top first too.
    pub scenes: Vec<SceneView>,
    pub program: String,
    pub preview: Option<String>,
    pub studio_mode: bool,
    pub streaming: bool,
    pub recording: bool,
    pub stats: Stats,
    /// Inputs with audio.
    pub audio: Vec<AudioView>,
    pub meter_extra: usize,
    pub meter_interval: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneView {
    pub name: String,
    pub items: Vec<ItemView>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ItemView {
    pub source: String,
    pub enabled: bool,
    pub locked: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioView {
    pub name: String,
    pub volume_db: f64,
    pub muted: bool,
    /// The level the audio meters send, 0–1.
    pub level: f64,
}

/// What `GetStats` and `GetStreamStatus` report; the frame and byte counters grow with time from these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub cpu_percent: f64,
    /// Share of rendered (and, while streaming, sent) frames that are dropped.
    pub dropped_percent: f64,
    /// The stream output's bitrate while streaming.
    pub bitrate_kbps: u64,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            cpu_percent: 2.5,
            dropped_percent: 0.0,
            bitrate_kbps: 6000,
        }
    }
}

struct Counters {
    render_total: f64,
    render_skipped: f64,
    output_total: f64,
    output_skipped: f64,
    output_bytes: f64,
    at: Instant,
}

const FPS: f64 = 60.0;

/// The scene `obs sources <n>` fills, and its sources' name prefix.
const STRESS: &str = "Stress";
/// The most sources the stress scene holds.
const STRESS_MAX: usize = 1000;

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
    /// A collection like a real stream's: 12 scenes, 40 sources (8 with audio), 2 groups, 2 scenes used inside other
    /// scenes, and some items hidden or locked. "Starting", "Live" and "Break" stay small for the tests.
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
            stats: Stats::default(),
            meter_interval: Duration::from_millis(50),
            meter_extra: 0,
            counters: Counters {
                render_total: 0.0,
                render_skipped: 0.0,
                output_total: 0.0,
                output_skipped: 0.0,
                output_bytes: 0.0,
                at: Instant::now(),
            },
        };
        let overlay = |path: &str, width: u32, height: u32| json!({"url": format!("http://127.0.0.1:47811/overlay/{path}/"), "width": width, "height": height});
        let audio = [
            ("Mic", "wasapi_input_capture"),
            ("Desktop Audio", "wasapi_output_capture"),
            ("Guest Mic", "wasapi_input_capture"),
            ("Game Audio", "wasapi_process_output_capture"),
            ("Music", "wasapi_process_output_capture"),
            ("Discord", "wasapi_process_output_capture"),
            ("Intro Music", "ffmpeg_source"),
            ("BRB Music", "ffmpeg_source"),
        ]
        .map(|(name, kind)| (name, kind, true, json!({})));
        let video = [
            ("Camera", "dshow_input"),
            ("Guest Camera", "dshow_input"),
            ("Capture Card", "dshow_input"),
            ("Game Capture", "game_capture"),
            ("Display Capture", "monitor_capture"),
            ("Browser Window", "window_capture"),
            ("Logo", "image_source"),
            ("Frame", "image_source"),
            ("Background", "image_source"),
            ("Starting Background", "image_source"),
            ("Ending Background", "image_source"),
            ("Webcam Border", "image_source"),
            ("Sponsor Banner", "image_source"),
            ("Black", "color_source_v3"),
            ("Title Text", "text_gdiplus_v3"),
            ("Now Playing", "text_gdiplus_v3"),
            ("Starting Soon Text", "text_gdiplus_v3"),
            ("Countdown", "text_gdiplus_v3"),
            ("BRB Text", "text_gdiplus_v3"),
            ("Ending Text", "text_gdiplus_v3"),
            ("Socials Text", "text_gdiplus_v3"),
        ]
        .map(|(name, kind)| (name, kind, false, json!({})));
        let overlays = [
            ("Chat", overlay("chat", 400, 600)),
            ("Alert Box", overlay("alerts", 800, 600)),
            ("Check-in", overlay("checkin", 800, 600)),
            ("Leaderboard", overlay("leaderboard", 400, 600)),
            ("Poll", overlay("polls", 800, 400)),
            ("Counter", overlay("counter", 400, 200)),
            ("Marathon", overlay("marathon", 400, 200)),
            ("Break Timer", overlay("break", 800, 400)),
            ("Warnings", overlay("warnings", 800, 200)),
            ("Sound", overlay("sound", 400, 200)),
            ("BGM", overlay("bgm", 400, 200)),
        ]
        .map(|(name, settings)| (name, "browser_source", false, settings));
        for (name, kind, audio, settings) in audio.into_iter().chain(video).chain(overlays) {
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
        // Top first, each scene's items bottom first; a leading `-` hides an item, `#` locks it.
        let scenes: [(&str, bool, &[&str]); 14] = [
            (
                "Starting",
                false,
                &[
                    "Starting Background",
                    "Starting Soon Text",
                    "Countdown",
                    "Intro Music",
                    "Logo",
                ],
            ),
            ("Live", false, &["Camera", "Chat", "Logo"]),
            (
                "Game",
                false,
                &[
                    "Game Capture",
                    "Game Audio",
                    "#Frame",
                    "Webcam",
                    "Now Playing",
                    "Alerts Layer",
                ],
            ),
            (
                "Just Chatting",
                false,
                &[
                    "#Background",
                    "Camera",
                    "Chat",
                    "Title Text",
                    "Music",
                    "Sponsors",
                    "Alerts Layer",
                ],
            ),
            (
                "Desktop",
                false,
                &[
                    "Display Capture",
                    "-Browser Window",
                    "Music",
                    "Alerts Layer",
                ],
            ),
            (
                "Console",
                false,
                &["Capture Card", "Game Audio", "#Frame", "Alerts Layer"],
            ),
            (
                "Guest",
                false,
                &["Guest Camera", "Camera", "Guest Mic", "Discord", "-Chat"],
            ),
            ("Break", false, &["Logo"]),
            (
                "BRB",
                false,
                &["#Black", "BRB Text", "Break Timer", "BRB Music", "Logo"],
            ),
            (
                "Ending",
                false,
                &[
                    "Ending Background",
                    "Ending Text",
                    "Socials Text",
                    "Sponsors",
                ],
            ),
            (
                "Alerts Layer",
                false,
                &["Overlays", "-Warnings", "Sound", "BGM", "Alert Box"],
            ),
            ("Sponsors", false, &["Sponsor Banner", "Socials Text"]),
            ("Webcam", true, &["Camera", "Webcam Border"]),
            (
                "Overlays",
                true,
                &["Check-in", "Leaderboard", "Poll", "Counter", "Marathon"],
            ),
        ];
        for (name, group, sources) in scenes {
            let uuid = w.uuid();
            let items = sources
                .iter()
                .map(|s| {
                    let source = s.trim_start_matches(['-', '#']);
                    let mut item = w.item(source);
                    item.enabled = !s.starts_with('-');
                    item.locked = s.starts_with('#');
                    item
                })
                .collect();
            w.scenes.push(Scene {
                name: name.into(),
                uuid,
                group,
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
            transform: json!({}),
        }
    }

    /// Frames and bytes since the last read, at `FPS` and the set drop share and bitrate.
    fn tick(&mut self) {
        let now = Instant::now();
        let secs = now.duration_since(self.counters.at).as_secs_f64();
        self.counters.at = now;
        let frames = secs * FPS;
        let dropped = frames * self.stats.dropped_percent / 100.0;
        let c = &mut self.counters;
        c.render_total += frames;
        c.render_skipped += dropped;
        if self.streaming {
            c.output_total += frames;
            c.output_skipped += dropped;
            c.output_bytes += self.stats.bitrate_kbps as f64 * 1000.0 / 8.0 * secs;
        }
    }

    /// `(width, height)` of a source as OBS reports it before scaling.
    fn source_size(&self, name: &str) -> (f64, f64) {
        if let Some(input) = self.inputs.iter().find(|i| i.name == name) {
            return match input.kind.as_str() {
                "dshow_input" => (1920.0, 1080.0),
                "image_source" => (300.0, 100.0),
                "browser_source" => (
                    input.settings["width"].as_f64().unwrap_or(800.0),
                    input.settings["height"].as_f64().unwrap_or(600.0),
                ),
                _ => (0.0, 0.0),
            };
        }
        (1920.0, 1080.0)
    }

    fn transform(&self, scene: usize, item: usize) -> Value {
        let it = &self.scenes[scene].items[item];
        let (w, h) = self.source_size(&it.source);
        let mut t = json!({"sourceWidth": w, "sourceHeight": h, "positionX": 0.0, "positionY": 0.0,
            "rotation": 0.0, "scaleX": 1.0, "scaleY": 1.0, "alignment": 5, "boundsType": "OBS_BOUNDS_NONE",
            "boundsAlignment": 0, "boundsWidth": 0.0, "boundsHeight": 0.0,
            "cropLeft": 0, "cropRight": 0, "cropTop": 0, "cropBottom": 0});
        if let (Some(t), Some(set)) = (t.as_object_mut(), it.transform.as_object()) {
            t.extend(set.clone());
        }
        let sx = t["scaleX"].as_f64().unwrap_or(1.0).abs();
        let sy = t["scaleY"].as_f64().unwrap_or(1.0).abs();
        t["width"] = json!(w * sx);
        t["height"] = json!(h * sy);
        t
    }

    fn meters(&self) -> Value {
        let level = |muted: bool, mul: f64| {
            let level = if muted { 0.0 } else { 0.25 * mul };
            let peak = (level * 1.2).min(1.0);
            json!([[level, peak, peak], [level, peak, peak]])
        };
        let mut inputs: Vec<Value> = self
            .inputs
            .iter()
            .filter(|i| i.audio)
            .map(|i| json!({"inputName": i.name, "inputUuid": i.uuid, "inputLevelsMul": level(i.muted, i.volume_mul)}))
            .collect();
        // Load-run inputs: in the meters only, as levels for sources the app has not read.
        inputs.extend((1..=self.meter_extra).map(|n| {
            json!({"inputName": format!("Bench Input {n}"), "inputUuid": uuid(1_000_000 + n as u64),
                   "inputLevelsMul": level(false, 1.0)})
        }));
        json!({ "inputs": inputs })
    }

    fn input_event(&self, input: usize, field: &str, value: Value) -> Value {
        let i = &self.inputs[input];
        let mut event = json!({"inputName": i.name, "inputUuid": i.uuid});
        event[field] = value;
        event
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
                self.tick();
                let c = &self.counters;
                json!({"cpuUsage": self.stats.cpu_percent, "memoryUsage": 300.0,
                "availableDiskSpace": 100000.0, "activeFps": FPS, "averageFrameRenderTime": 2.0,
                "renderSkippedFrames": c.render_skipped as u32, "renderTotalFrames": c.render_total as u32,
                "outputSkippedFrames": c.output_skipped as u32, "outputTotalFrames": c.output_total as u32,
                "webSocketSessionIncomingMessages": 0, "webSocketSessionOutgoingMessages": 0})
            }
            "GetStreamStatus" => {
                self.tick();
                let c = &self.counters;
                json!({"outputActive": self.streaming, "outputReconnecting": false,
                "outputTimecode": "00:00:00.000", "outputDuration": 0, "outputCongestion": 0.0,
                "outputBytes": c.output_bytes as u64, "outputSkippedFrames": c.output_skipped as u32,
                "outputTotalFrames": c.output_total as u32})
            }
            "GetVideoSettings" => {
                json!({"fpsNumerator": 60, "fpsDenominator": 1, "baseWidth": 1920,
                "baseHeight": 1080, "outputWidth": 1920, "outputHeight": 1080})
            }
            "GetSceneItemTransform" => {
                let scene = self.scene_index(data)?;
                let item = self.item_index(scene, data)?;
                json!({ "sceneItemTransform": self.transform(scene, item) })
            }
            "SetSceneItemTransform" => {
                let scene = self.scene_index(data)?;
                let item = self.item_index(scene, data)?;
                let set = data["sceneItemTransform"]
                    .as_object()
                    .ok_or((MISSING_REQUEST_FIELD, "sceneItemTransform".into()))?;
                // Sizes follow from the source and the scale; OBS ignores them here too.
                let kept = set.iter().filter(|(k, _)| {
                    !matches!(
                        k.as_str(),
                        "width" | "height" | "sourceWidth" | "sourceHeight"
                    )
                });
                if let Some(t) = self.scenes[scene].items[item].transform.as_object_mut() {
                    t.extend(kept.map(|(k, v)| (k.clone(), v.clone())));
                }
                Value::Null
            }
            "SetInputVolume" => {
                let input = self.audio_input(data)?;
                let mul = match (
                    data["inputVolumeMul"].as_f64(),
                    data["inputVolumeDb"].as_f64(),
                ) {
                    (Some(mul), _) => mul.clamp(0.0, 20.0),
                    (None, Some(db)) => 10f64.powf(db.clamp(-100.0, 26.0) / 20.0),
                    _ => {
                        return Err((
                            MISSING_REQUEST_FIELD,
                            "inputVolumeMul or inputVolumeDb".into(),
                        ));
                    }
                };
                self.inputs[input].volume_mul = mul;
                let mut event = self.input_event(input, "inputVolumeMul", json!(mul));
                event["inputVolumeDb"] = json!(db(mul));
                events.push(("InputVolumeChanged", event));
                Value::Null
            }
            "SetInputMute" => {
                let input = self.audio_input(data)?;
                let muted = data["inputMuted"]
                    .as_bool()
                    .ok_or((MISSING_REQUEST_FIELD, "inputMuted".into()))?;
                self.inputs[input].muted = muted;
                let event = self.input_event(input, "inputMuted", json!(muted));
                events.push(("InputMuteStateChanged", event));
                Value::Null
            }
            "SetInputAudioMonitorType" => {
                let input = self.audio_input(data)?;
                let monitor = match data["monitorType"].as_str() {
                    Some("OBS_MONITORING_TYPE_NONE") => "OBS_MONITORING_TYPE_NONE",
                    Some("OBS_MONITORING_TYPE_MONITOR_ONLY") => "OBS_MONITORING_TYPE_MONITOR_ONLY",
                    Some("OBS_MONITORING_TYPE_MONITOR_AND_OUTPUT") => {
                        "OBS_MONITORING_TYPE_MONITOR_AND_OUTPUT"
                    }
                    _ => return Err((MISSING_REQUEST_FIELD, "monitorType".into())),
                };
                self.inputs[input].monitor = monitor;
                let event = self.input_event(input, "monitorType", json!(monitor));
                events.push(("InputAudioMonitorTypeChanged", event));
                Value::Null
            }
            "GetInputDefaultSettings" => {
                let kind = data["inputKind"]
                    .as_str()
                    .ok_or((MISSING_REQUEST_FIELD, "inputKind".into()))?;
                let defaults = match kind {
                    "browser_source" => json!({"url": "", "width": 800, "height": 600, "css": "",
                        "shutdown": false, "restart_when_active": false, "reroute_audio": false}),
                    _ => json!({}),
                };
                json!({ "defaultInputSettings": defaults })
            }
            "SetInputSettings" => {
                let input = self.input_index(data)?;
                let set = data["inputSettings"]
                    .as_object()
                    .ok_or((MISSING_REQUEST_FIELD, "inputSettings".into()))?
                    .clone();
                let settings = &mut self.inputs[input].settings;
                // `overlay` defaults to true: merge into the current settings instead of replacing them.
                if data["overlay"].as_bool() == Some(false) {
                    *settings = Value::Object(set);
                } else if let Some(current) = settings.as_object_mut() {
                    current.extend(set);
                }
                Value::Null
            }
            "GetInputPropertiesListPropertyItems" => {
                let input = self.input_index(data)?;
                let kind = self.inputs[input].kind.as_str();
                let items = match (kind, data["propertyName"].as_str()) {
                    ("dshow_input", Some("video_device_id")) => json!([
                        {"itemName": "Fake Camera", "itemEnabled": true, "itemValue": "fake-camera-1"},
                        {"itemName": "Busy Camera", "itemEnabled": false, "itemValue": "fake-camera-2"}]),
                    _ => json!([]),
                };
                json!({ "propertyItems": items })
            }
            "PressInputPropertiesButton" => {
                self.input_index(data)?;
                Value::Null
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

    /// Creates, grows, shrinks or (at 0) removes the stress scene so it holds `n` color sources, with the events OBS
    /// sends for each change. Events of the changes made before a failure stay in `events`.
    fn set_stress_sources(&mut self, n: usize, events: &mut Vec<Event>) -> Result<(), String> {
        let n = n.min(STRESS_MAX);
        let scene = match self.scene_by_name(STRESS) {
            Some(scene) => scene,
            None if n == 0 => return Ok(()),
            None => {
                let uuid = self.uuid();
                // A new scene goes to the bottom of OBS's list; groups are no scenes there.
                let at = self
                    .scenes
                    .iter()
                    .position(|s| s.group)
                    .unwrap_or(self.scenes.len());
                self.scenes.insert(
                    at,
                    Scene {
                        name: STRESS.into(),
                        uuid: uuid.clone(),
                        group: false,
                        items: Vec::new(),
                    },
                );
                events.push((
                    "SceneCreated",
                    json!({"sceneName": STRESS, "sceneUuid": uuid, "isGroup": false}),
                ));
                at
            }
        };
        // shortcut: counts every item of the stress scene as its own; one added there by hand counts too.
        let have = self.scenes[scene].items.len();
        for k in have + 1..=n {
            let data = json!({"sceneName": STRESS, "inputName": format!("{STRESS} {k}"),
                              "inputKind": "color_source_v3"});
            self.apply("CreateInput", data, events)?;
        }
        for _ in n..have {
            let Some(item) = self.scenes[scene].items.last().cloned() else {
                break;
            };
            self.apply(
                "RemoveSceneItem",
                json!({"sceneName": STRESS, "sceneItemId": item.id}),
                events,
            )?;
            self.drop_unused_input(&item.source, events);
        }
        if n > 0 {
            return Ok(());
        }
        // OBS leaves no current scene pointing at a removed one, and takes it out of every scene that holds it.
        let top = self
            .scenes
            .iter()
            .find(|s| !s.group && s.name != STRESS)
            .map(|s| s.name.clone())
            .ok_or("no other scene to switch to")?;
        if self.program == STRESS {
            self.apply(
                "SetCurrentProgramScene",
                json!({ "sceneName": top }),
                events,
            )?;
        }
        if self.preview.as_deref() == Some(STRESS) {
            self.apply(
                "SetCurrentPreviewScene",
                json!({ "sceneName": top }),
                events,
            )?;
        }
        let holders: Vec<(String, u64)> = self
            .scenes
            .iter()
            .flat_map(|s| {
                s.items
                    .iter()
                    .filter(|i| i.source == STRESS)
                    .map(|i| (s.name.clone(), i.id))
            })
            .collect();
        for (holder, id) in holders {
            self.apply(
                "RemoveSceneItem",
                json!({"sceneName": holder, "sceneItemId": id}),
                events,
            )?;
        }
        let scene = self
            .scene_by_name(STRESS)
            .ok_or("the stress scene is gone")?;
        let removed = self.scenes.remove(scene);
        events.push((
            "SceneRemoved",
            json!({"sceneName": removed.name, "sceneUuid": removed.uuid, "isGroup": false}),
        ));
        Ok(())
    }

    /// Answers `request` as a connection would, keeping its events.
    fn apply(&mut self, request: &str, data: Value, events: &mut Vec<Event>) -> Result<(), String> {
        let (result, mut changed) = self.handle(request, &data);
        events.append(&mut changed);
        result.map(drop).map_err(|(_, e)| e)
    }

    /// Removes input `name` once no scene holds it, as OBS drops a source nothing uses, with `InputRemoved`.
    fn drop_unused_input(&mut self, name: &str, events: &mut Vec<Event>) {
        let used = self
            .scenes
            .iter()
            .any(|s| s.items.iter().any(|i| i.source == name));
        if let Some(at) = self
            .inputs
            .iter()
            .position(|i| i.name == name)
            .filter(|_| !used)
        {
            let input = self.inputs.remove(at);
            events.push((
                "InputRemoved",
                json!({"inputName": input.name, "inputUuid": input.uuid}),
            ));
        }
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
    /// Closes every connection, like OBS exiting.
    close: broadcast::Sender<()>,
    password: Option<String>,
    recorder: Recorder,
    open: Arc<AtomicU32>,
}

/// Counts a connection as open until dropped.
struct Open(Arc<AtomicU32>);

impl Drop for Open {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

impl FakeObs {
    /// Without a password the fake asks for no authentication.
    pub fn new(password: Option<String>) -> Self {
        Self::with_recorder(password, Recorder::default())
    }

    /// Logs requests and events into `recorder`, shared with the fake Twitch for one timeline.
    pub fn with_recorder(password: Option<String>, recorder: Recorder) -> Self {
        Self {
            world: Arc::new(Mutex::new(World::sample())),
            events: broadcast::channel(1024).0,
            close: broadcast::channel(1).0,
            password,
            recorder,
            open: Arc::default(),
        }
    }

    pub fn recorder(&self) -> &Recorder {
        &self.recorder
    }

    /// Adds inputs before a client connects, without events (e.g. a scene collection with many sources).
    pub fn add_inputs(&self, names: &[&str], kind: &str) {
        let mut w = self.world.lock().unwrap();
        for name in names {
            let uuid = w.uuid();
            w.inputs.push(Input {
                name: (*name).into(),
                uuid,
                kind: kind.into(),
                settings: json!({}),
                audio: kind.starts_with("wasapi_"),
                volume_mul: 1.0,
                muted: false,
                monitor: "OBS_MONITORING_TYPE_NONE",
            });
        }
    }

    /// Sends an event to every connection and logs it.
    fn emit(&self, event: Event) {
        self.recorder.event("obs", event.0);
        // No connection listening is fine.
        let _ = self.events.send(event);
    }

    /// Closes every connection as OBS does when it exits; clients may connect again right away.
    pub fn drop_connections(&self) {
        let _ = self.close.send(());
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/", any(session))
            .with_state(self.clone())
    }

    /// Serves until the listener fails; bind it to `127.0.0.1` only.
    /// Also sends audio levels every 50 ms, as OBS does.
    pub async fn serve(self, listener: TcpListener) -> std::io::Result<()> {
        tokio::spawn(meters(self.clone()));
        axum::serve(listener, self.router()).await
    }

    /// Audio meters every `interval` (at least 10 ms) with `extra` more inputs than the scene collection has.
    pub fn set_meters(&self, extra: usize, interval: Duration) {
        let mut w = self.world.lock().unwrap();
        w.meter_extra = extra.min(1000);
        w.meter_interval = interval.max(Duration::from_millis(10));
    }

    /// Changes OBS makes on its own, `count` times, with the events each sends: `scenes` switches the program scene
    /// back and forth; `items` adds a logo to "Live" and removes it again. The app re-reads its state after each.
    pub fn snapshot(&self) -> ObsSnapshot {
        let w = self.world.lock().unwrap();
        ObsSnapshot {
            open: self.open.load(Ordering::Relaxed),
            scenes: w
                .scenes
                .iter()
                .filter(|s| !s.group)
                .map(|s| SceneView {
                    name: s.name.clone(),
                    items: s
                        .items
                        .iter()
                        .rev()
                        .map(|i| ItemView {
                            source: i.source.clone(),
                            enabled: i.enabled,
                            locked: i.locked,
                        })
                        .collect(),
                })
                .collect(),
            program: w.program.clone(),
            preview: w.preview.clone(),
            studio_mode: w.studio_mode,
            streaming: w.streaming,
            recording: w.recording,
            stats: w.stats,
            audio: w
                .inputs
                .iter()
                .filter(|i| i.audio)
                .map(|i| AudioView {
                    name: i.name.clone(),
                    volume_db: db(i.volume_mul),
                    muted: i.muted,
                    level: if i.muted { 0.0 } else { 0.25 * i.volume_mul },
                })
                .collect(),
            meter_extra: w.meter_extra,
            meter_interval: w.meter_interval,
        }
    }

    /// Turns studio mode on or off in OBS itself, with the event OBS sends.
    pub fn set_studio_mode(&self, enabled: bool) {
        let _ = self.request(
            "SetStudioModeEnabled",
            &json!({ "studioModeEnabled": enabled }),
        );
    }

    /// Switches the program scene in OBS itself (not through the app), with the event OBS sends.
    pub fn set_program(&self, scene: &str) -> Result<(), String> {
        self.request("SetCurrentProgramScene", &json!({ "sceneName": scene }))
            .map(drop)
            .map_err(|(_, e)| e)
    }

    pub fn burst(&self, kind: &str, count: usize) -> Result<(), String> {
        let names: Vec<String> = self
            .world
            .lock()
            .unwrap()
            .scenes
            .iter()
            .filter(|s| !s.group)
            .map(|s| s.name.clone())
            .collect();
        for n in 0..count.min(1000) {
            match kind {
                "scenes" => {
                    let scene = &names[n % names.len()];
                    self.request("SetCurrentProgramScene", &json!({ "sceneName": scene }))
                        .map_err(|(_, e)| e)?;
                }
                "items" => {
                    let created = self
                        .request(
                            "CreateSceneItem",
                            &json!({"sceneName": "Live", "sourceName": "Logo"}),
                        )
                        .map_err(|(_, e)| e)?;
                    let data = json!({"sceneName": "Live", "sceneItemId": created["sceneItemId"]});
                    self.request("RemoveSceneItem", &data).map_err(|(_, e)| e)?;
                }
                _ => return Err(format!("unknown burst {kind}: scenes or items")),
            }
        }
        Ok(())
    }

    /// Creates, resizes or (at 0) removes the "Stress" scene so it holds `n` sources (at most 1000), with the events
    /// OBS sends for each change, so the app sees it without reconnecting.
    pub fn set_stress_sources(&self, n: usize) -> Result<(), String> {
        let mut events = Vec::new();
        let result = self
            .world
            .lock()
            .unwrap()
            .set_stress_sources(n, &mut events);
        for event in events {
            self.emit(event);
        }
        result
    }

    /// How many sources the stress scene holds; 0 when there is none.
    pub fn stress_sources(&self) -> usize {
        let w = self.world.lock().unwrap();
        w.scene_by_name(STRESS)
            .map_or(0, |s| w.scenes[s].items.len())
    }

    /// What `GetStats` and `GetStreamStatus` report from now on.
    pub fn set_stats(&self, stats: Stats) {
        let mut w = self.world.lock().unwrap();
        w.tick();
        w.stats = stats;
    }

    pub fn stats(&self) -> Stats {
        self.world.lock().unwrap().stats
    }

    /// Starts or stops streaming and recording (the app never does), with the events OBS sends.
    pub fn set_output(&self, streaming: Option<bool>, recording: Option<bool>) {
        let state = |on: bool| {
            if on {
                "OBS_WEBSOCKET_OUTPUT_STARTED"
            } else {
                "OBS_WEBSOCKET_OUTPUT_STOPPED"
            }
        };
        let mut events = Vec::new();
        {
            let mut w = self.world.lock().unwrap();
            w.tick();
            if let Some(on) = streaming.filter(|on| *on != w.streaming) {
                w.streaming = on;
                let event = json!({"outputActive": on, "outputState": state(on)});
                events.push(("StreamStateChanged", event));
            }
            if let Some(on) = recording.filter(|on| *on != w.recording) {
                w.recording = on;
                let event =
                    json!({"outputActive": on, "outputState": state(on), "outputPath": null});
                events.push(("RecordStateChanged", event));
            }
        }
        for event in events {
            self.emit(event);
        }
    }

    /// Answers one request as a connection would, sending its events to every connection.
    fn request(&self, request: &str, data: &Value) -> Result<Value, Failure> {
        let (result, events) = self.world.lock().unwrap().handle(request, data);
        for event in events {
            self.emit(event);
        }
        result
    }
}

/// `InputVolumeMeters` at OBS's 50 ms (or the load run's interval), from the inputs' volume and mute (fixed levels,
/// no real audio).
async fn meters(fake: FakeObs) {
    loop {
        let every = fake.world.lock().unwrap().meter_interval;
        tokio::time::sleep(every).await;
        if fake.events.receiver_count() > 0 {
            let levels = fake.world.lock().unwrap().meters();
            fake.emit(("InputVolumeMeters", levels));
        }
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
    let upgrade = match crate::twitch::refused(&fake.recorder, "obs", "obs", upgrade).await {
        Ok(upgrade) => upgrade,
        Err(refused) => return refused,
    };
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
        fake.open.fetch_add(1, Ordering::Relaxed);
        let _open = Open(fake.open.clone());
        let mut events = fake.events.subscribe();
        let mut close = fake.close.subscribe();
        // Requests are answered in their own tasks, as OBS does: a slow one holds up neither the next requests,
        // nor events, nor closing.
        let (replies_tx, mut replies) = tokio::sync::mpsc::unbounded_channel::<Value>();
        loop {
            tokio::select! {
                _ = close.recv() => {
                    let frame = ws::CloseFrame { code: 1001, reason: "OBS is exiting".into() };
                    let _ = socket.send(ws::Message::Close(Some(frame))).await;
                    return;
                }
                message = socket.recv() => match message {
                    Some(Ok(ws::Message::Text(text))) => {
                        let message: Value = serde_json::from_str(text.as_str()).unwrap_or_default();
                        match message["op"].as_u64() {
                            // Reidentify.
                            Some(3) => {
                                let identified = json!({"op": 2, "d": {"negotiatedRpcVersion": 1}});
                                if !send(&mut socket, identified).await {
                                    return;
                                }
                            }
                            Some(6) => {
                                let (fake, tx) = (fake.clone(), replies_tx.clone());
                                tokio::spawn(async move {
                                    let _ = tx.send(reply(&fake, &message["d"]).await);
                                });
                            }
                            _ => {}
                        }
                    }
                    Some(Ok(_)) => {}
                    _ => return,
                },
                Some(reply) = replies.recv() => if !send(&mut socket, reply).await { return },
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
async fn reply(fake: &FakeObs, request: &Value) -> Value {
    let request_type = request["requestType"].as_str().unwrap_or_default();
    let data = request.get("requestData").cloned().unwrap_or(Value::Null);
    let in_flight = fake.recorder.start("obs");
    let started = Instant::now();
    let fault = fake.recorder.fault(request_type);
    if let Some(f) = fault {
        tokio::time::sleep(f.delay).await;
    }
    let answer = match fault.and_then(|f| f.status) {
        Some(code) => Err((code, "fake fault".to_owned())),
        None => fake.request(request_type, &data),
    };
    let code = answer.as_ref().map_or_else(|(code, _)| *code, |_| SUCCESS);
    fake.recorder.call(Call {
        at: started,
        service: "obs",
        key: request_type.into(),
        params: metrics::hash(&[data.to_string().as_bytes()]),
        status: code,
        handled: started.elapsed(),
        in_flight: in_flight.count,
        problem: None,
    });
    let (status, response) = match answer {
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
        assert_eq!(names.len(), 12, "groups are no scenes");
        assert_eq!(names.last(), Some(&"Starting"), "the top scene last");
        let items = w
            .handle("GetSceneItemList", &json!({"sceneName": "Live"}))
            .0
            .unwrap();
        assert_eq!(items["sceneItems"][0]["sourceName"], "Camera");
        assert_eq!(items["sceneItems"][1]["inputKind"], "browser_source");
    }

    #[test]
    fn the_default_collection_looks_like_a_real_stream() {
        let w = world();
        let scenes = w.scenes.iter().filter(|s| !s.group).count();
        let groups = w.scenes.iter().filter(|s| s.group).count();
        let audio = w.inputs.iter().filter(|i| i.audio).count();
        assert_eq!((scenes, groups, w.inputs.len(), audio), (12, 2, 40, 8));
        let items = || w.scenes.iter().flat_map(|s| &s.items);
        assert!(items().any(|i| !i.enabled) && items().any(|i| i.locked));
        let nested: std::collections::BTreeSet<&str> = items()
            .map(|i| i.source.as_str())
            .filter(|source| w.scenes.iter().any(|s| !s.group && s.name == *source))
            .collect();
        assert_eq!(
            nested.len(),
            2,
            "scenes used inside other scenes: {nested:?}"
        );
        for item in items() {
            assert!(w.source(&item.source).is_some(), "{} exists", item.source);
        }
    }

    #[test]
    fn the_stress_scene_grows_shrinks_and_goes_with_obs_events() {
        let mut w = world();
        let inputs = w.inputs.len();
        let count = |events: &[Event], kind: &str| events.iter().filter(|e| e.0 == kind).count();

        let mut events = Vec::new();
        w.set_stress_sources(300, &mut events).unwrap();
        let stress = w.scene_by_name(STRESS).unwrap();
        assert_eq!(w.scenes[stress].items.len(), 300);
        assert_eq!(w.inputs.len(), inputs + 300);
        assert_eq!(events[0].0, "SceneCreated");
        assert_eq!(events[0].1["isGroup"], false);
        assert_eq!(
            (
                count(&events, "InputCreated"),
                count(&events, "SceneItemCreated")
            ),
            (300, 300)
        );
        assert_eq!(events.len(), 601);

        w.handle("SetCurrentProgramScene", &json!({ "sceneName": STRESS }))
            .0
            .unwrap();
        let mut events = Vec::new();
        w.set_stress_sources(50, &mut events).unwrap();
        assert_eq!(w.scenes[stress].items.len(), 50);
        assert_eq!(w.scenes[stress].items.last().unwrap().source, "Stress 50");
        assert_eq!(
            (
                count(&events, "SceneItemRemoved"),
                count(&events, "InputRemoved")
            ),
            (250, 250)
        );
        assert_eq!(events.len(), 500);

        let mut events = Vec::new();
        w.set_stress_sources(0, &mut events).unwrap();
        assert!(w.scene_by_name(STRESS).is_none());
        assert_eq!(w.inputs.len(), inputs);
        assert_eq!(
            w.program, "Starting",
            "the program scene moves off the removed one"
        );
        assert_eq!(
            (
                count(&events, "SceneItemRemoved"),
                count(&events, "InputRemoved")
            ),
            (50, 50)
        );
        assert_eq!(count(&events, "CurrentProgramSceneChanged"), 1);
        assert_eq!(events.last().unwrap().0, "SceneRemoved");
        assert_eq!(events.len(), 102);

        let mut events = Vec::new();
        w.set_stress_sources(0, &mut events).unwrap();
        assert!(events.is_empty(), "nothing to remove");
        w.set_stress_sources(5000, &mut events).unwrap();
        assert_eq!(
            w.scenes[w.scene_by_name(STRESS).unwrap()].items.len(),
            STRESS_MAX
        );
    }

    #[test]
    fn scene_bursts_switch_between_scenes_only() {
        for count in 1..=16 {
            let obs = FakeObs::new(None);
            obs.burst("scenes", count).unwrap();
            let snap = obs.snapshot();
            assert!(
                snap.scenes.iter().any(|s| s.name == snap.program),
                "{} is a scene",
                snap.program
            );
        }
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
        let live = w.scene_by_name("Live").unwrap();
        let camera = w.scenes[live].items[0].id;
        let (result, events) = w.handle(
            "SetSceneItemIndex",
            &json!({"sceneName": "Live", "sceneItemId": camera, "sceneItemIndex": 2}),
        );
        result.unwrap();
        assert_eq!(w.scenes[live].items[2].source, "Camera");
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
