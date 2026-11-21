//! Rehearsal scripts (演練): a JSON list of simulator steps played on a timeline, at normal speed, twice as fast or
//! all at once. Every step goes through the simulator, so it is `source=simulation` whatever the file says, and no
//! module records it (chat reaches the chat display only, alerts the alerts overlay only, cards nothing). One script
//! runs at a time; a new one or 停止 ends the one running. Admin routes; the caller adds the token check.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use serde::{Deserialize, Serialize};
use toolkit_core::{bus::EventBus, presentation::Presenter};
use utoipa::ToSchema;

use crate::{
    admin::{
        AdminState, ErrorBody, error,
        simulate::{SimulatedAlert, send_alert, send_chat, send_checkin},
    },
    queue,
};

/// The largest script file accepted, in bytes of JSON text.
pub(crate) const MAX_SCRIPT_BYTES: usize = 256 * 1024;
const MAX_STEPS: usize = 1000;
/// The latest a step may start: half an hour.
const MAX_AT_MS: u64 = 30 * 60 * 1000;

/// One step; `at` is when it plays, in ms from the start. Fields a step does not use (`source` included) are ignored.
#[derive(Deserialize)]
// Fields are camelCase (`userName`), as the alert step's and the admin UI's scripts are.
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
enum Step {
    Chat {
        at: u64,
        #[serde(default)]
        user_name: String,
        text: String,
    },
    Alert {
        at: u64,
        #[serde(flatten)]
        alert: SimulatedAlert,
    },
    Checkin {
        at: u64,
        #[serde(default)]
        user_name: String,
        stamps: u32,
    },
    /// The emergency stop, to rehearse it.
    Stop { at: u64 },
}

impl Step {
    fn at(&self) -> u64 {
        match self {
            Self::Chat { at, .. }
            | Self::Alert { at, .. }
            | Self::Checkin { at, .. }
            | Self::Stop { at } => *at,
        }
    }
}

#[derive(Deserialize)]
struct Script {
    steps: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Speed {
    Normal,
    Double,
    /// Every step at once, in order.
    Burst,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct RunScript {
    pub(crate) speed: Speed,
    /// The script file's text: `{"steps": [{"type": "chat" | "alert" | "checkin" | "stop", "at": <ms>, …}]}`.
    pub(crate) script: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub(crate) struct ScriptStarted {
    pub(crate) steps: usize,
    /// When the last step plays, at the chosen speed.
    pub(crate) duration_ms: u64,
}

/// Why a script was refused: the line when the JSON itself is broken, or the step that does not fit.
#[derive(Debug, Serialize, ToSchema, PartialEq, Eq)]
pub(crate) struct ScriptRefused {
    /// `rehearsal.invalid` or `rehearsal.too_large`.
    pub(crate) code: String,
    /// 1-based line in the file of broken JSON.
    pub(crate) line: Option<usize>,
    /// 1-based number of a step that is not a known kind or misses a field.
    pub(crate) step: Option<usize>,
}

/// Where a script is wrong.
#[derive(Debug, Default, PartialEq, Eq)]
struct Wrong {
    line: Option<usize>,
    step: Option<usize>,
}

#[derive(Clone)]
pub(crate) struct RehearsalState {
    pub(crate) admin: Arc<AdminState>,
    pub(crate) presenter: Arc<Presenter>,
    pub(crate) bus: Arc<EventBus>,
    /// The script running, to stop it.
    pub(crate) running: Arc<Mutex<Option<tokio::task::AbortHandle>>>,
}

pub(crate) fn routes(state: RehearsalState) -> Router {
    Router::new()
        .route(
            "/api/admin/simulate/script",
            // The script is a JSON string inside the body, so its quotes, backslashes and line breaks double.
            post(run_script).layer(DefaultBodyLimit::max(2 * MAX_SCRIPT_BYTES + 1024)),
        )
        .route("/api/admin/simulate/script/stop", post(stop_script))
        .with_state(state)
}

fn refused(status: StatusCode, code: &str, wrong: Wrong) -> Response {
    (
        status,
        Json(ScriptRefused {
            code: code.into(),
            line: wrong.line,
            step: wrong.step,
        }),
    )
        .into_response()
}

/// Reads a script; the error names the line for broken JSON or a step that does not fit.
fn parse(text: &str) -> Result<Vec<Step>, Wrong> {
    let script: Script = serde_json::from_str(text).map_err(|e| Wrong {
        line: Some(e.line()),
        step: None,
    })?;
    if script.steps.len() > MAX_STEPS {
        return Err(Wrong::default());
    }
    script
        .steps
        .into_iter()
        .enumerate()
        .map(|(i, v)| {
            serde_json::from_value::<Step>(v)
                .ok()
                .filter(|s| s.at() <= MAX_AT_MS)
                .ok_or(Wrong {
                    line: None,
                    step: Some(i + 1),
                })
        })
        .collect()
}

fn scaled(at: u64, speed: Speed) -> u64 {
    match speed {
        Speed::Normal => at,
        Speed::Double => at / 2,
        Speed::Burst => 0,
    }
}

/// Plays a script (see the module docs); it replaces one still running.
#[utoipa::path(
    post,
    path = "/api/admin/simulate/script",
    tag = "admin",
    request_body = RunScript,
    responses(
        (status = 200, body = ScriptStarted),
        (status = 400, description = "`rehearsal.invalid`, with the line when the JSON is wrong", body = ScriptRefused),
        (status = 413, description = "`rehearsal.too_large`: more than 256 KB", body = ScriptRefused),
    )
)]
pub(crate) async fn run_script(
    State(s): State<RehearsalState>,
    body: Result<Json<RunScript>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return refused(
            StatusCode::BAD_REQUEST,
            "rehearsal.invalid",
            Wrong::default(),
        );
    };
    if body.script.len() > MAX_SCRIPT_BYTES {
        return refused(
            StatusCode::PAYLOAD_TOO_LARGE,
            "rehearsal.too_large",
            Wrong::default(),
        );
    }
    let mut steps = match parse(&body.script) {
        Ok(steps) => steps,
        Err(wrong) => return refused(StatusCode::BAD_REQUEST, "rehearsal.invalid", wrong),
    };
    // Stable: steps at the same time keep the file's order.
    steps.sort_by_key(Step::at);
    let duration_ms = steps.last().map_or(0, |s| scaled(s.at(), body.speed));
    let started = ScriptStarted {
        steps: steps.len(),
        duration_ms,
    };
    let speed = body.speed;
    let run = s.clone();
    let task = tokio::spawn(async move {
        let start = tokio::time::Instant::now();
        for step in steps {
            tokio::time::sleep_until(start + Duration::from_millis(scaled(step.at(), speed))).await;
            play(&run, step);
        }
    });
    if let Some(previous) = s.running.lock().unwrap().replace(task.abort_handle()) {
        previous.abort();
    }
    Json(started).into_response()
}

/// One step through the simulator; a step the simulator refuses is skipped and the rest go on.
fn play(s: &RehearsalState, step: Step) {
    let result = match step {
        Step::Chat {
            user_name, text, ..
        } => send_chat(&s.admin, &user_name, &text),
        Step::Alert { alert, .. } => send_alert(&s.admin, alert),
        Step::Checkin {
            user_name, stamps, ..
        } => send_checkin(&s.admin, &user_name, stamps),
        Step::Stop { .. } => {
            queue::stop_all(&s.presenter, &s.bus);
            Ok(())
        }
    };
    if let Err(code) = result {
        tracing::debug!(code, "rehearsal step skipped");
    }
}

/// Stops the script running; what it already sent plays on (the emergency stop clears that).
#[utoipa::path(
    post,
    path = "/api/admin/simulate/script/stop",
    tag = "admin",
    responses((status = 204), (status = 404, description = "`rehearsal.not_running`", body = ErrorBody))
)]
pub(crate) async fn stop_script(State(s): State<RehearsalState>) -> Response {
    match s.running.lock().unwrap().take() {
        Some(task) if !task.is_finished() => {
            task.abort();
            StatusCode::NO_CONTENT.into_response()
        }
        _ => error(StatusCode::NOT_FOUND, "rehearsal.not_running"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_name_the_broken_line_or_step_and_ignore_source() {
        let broken = "{\n  \"steps\": [\n    {\"type\": \"chat\", \"at\": 0,\n  ]\n}";
        assert_eq!(parse(broken).err().unwrap().line, Some(4));
        let unknown = "{\"steps\": [{\"type\": \"chat\", \"at\": 0, \"text\": \"hi\"}, {\"type\": \"nope\", \"at\": 1}]}";
        assert_eq!(parse(unknown).err().unwrap().step, Some(2));
        let ok = "{\"steps\": [{\"type\": \"chat\", \"at\": 0, \"text\": \"hi\", \"source\": \"twitch\"}, {\"type\": \"stop\", \"at\": 5}]}";
        assert_eq!(parse(ok).unwrap().len(), 2);
        let late = format!(
            "{{\"steps\": [{{\"type\": \"stop\", \"at\": {}}}]}}",
            MAX_AT_MS + 1
        );
        assert_eq!(parse(&late).err().unwrap().step, Some(1));
    }

    #[test]
    fn chat_and_checkin_steps_read_the_user_name() {
        let text = r#"{"steps": [
            {"type": "chat", "at": 0, "userName": "Ann", "text": "hi"},
            {"type": "checkin", "at": 1, "userName": "Bo", "stamps": 2}
        ]}"#;
        let names: Vec<_> = parse(text)
            .unwrap()
            .into_iter()
            .map(|s| match s {
                Step::Chat { user_name, .. } | Step::Checkin { user_name, .. } => user_name,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(names, ["Ann", "Bo"]);
    }

    #[test]
    fn speeds_scale_the_timeline() {
        assert_eq!(scaled(1000, Speed::Normal), 1000);
        assert_eq!(scaled(1000, Speed::Double), 500);
        assert_eq!(scaled(1000, Speed::Burst), 0);
    }
}
