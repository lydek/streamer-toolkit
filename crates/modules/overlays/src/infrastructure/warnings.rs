//! The on-stream warnings' admin API and the `screen.warnings` event.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use toolkit_core::events::Source;
use utoipa::ToSchema;

use super::{ErrorBody, Overlays, db, error, storage_error};
use crate::{
    application::OverlayError,
    domain::warnings::{WarningSettings, Warnings},
};
use toolkit_core::time::Timestamp;

pub(crate) const WARNINGS: &str = "screen.warnings";
pub(crate) const WARNINGS_VERSION: u32 = 1;

/// Which warnings are on and what was last seen. The overlay shows a warning that is on and down; the mic one only
/// once `micMutedSince` is `micSeconds` old.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WarningsPayload {
    pub chat_overlay: bool,
    pub twitch: bool,
    pub mic: bool,
    pub mic_seconds: u32,
    /// The OBS audio input watched; `null`: the mic is not checked.
    pub mic_input: Option<String>,
    /// No chat overlay is open in OBS.
    pub chat_overlay_down: bool,
    /// The Twitch event connection is down.
    pub twitch_down: bool,
    /// UTC, RFC 3339: since when the watched input has been muted.
    pub mic_muted_since: Option<String>,
    /// The watched input is no longer in OBS: pick another.
    pub mic_missing: bool,
}

impl WarningsPayload {
    pub(crate) fn new(s: &WarningSettings, w: &Warnings) -> Self {
        Self {
            chat_overlay: s.chat_overlay,
            twitch: s.twitch,
            mic: s.mic,
            mic_seconds: s.mic_seconds,
            mic_input: s.mic_input.clone(),
            chat_overlay_down: w.chat_overlay_down,
            twitch_down: w.twitch_down,
            mic_muted_since: w.mic_muted_since.map(db::rfc3339),
            mic_missing: w.mic_missing,
        }
    }
}

/// A `screen.warnings` event exactly as `/ws/overlay` sends it after any change and at start; the snapshot keeps
/// only the latest one.
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WarningsEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: WarningsType,
    pub version: u32,
    pub seq: u64,
    pub occurred_at: String,
    pub source: Source,
    pub payload: WarningsPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub enum WarningsType {
    #[serde(rename = "screen.warnings")]
    ScreenWarnings,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct WarningSettingsBody {
    pub chat_overlay: bool,
    pub twitch: bool,
    pub mic: bool,
    /// 1–3600.
    pub mic_seconds: u32,
    /// An OBS audio input name; `null` or blank: the mic is not checked.
    pub mic_input: Option<String>,
}

pub(crate) fn routes() -> Router<Arc<Overlays>> {
    Router::new().route(
        "/api/admin/screen-tools/warnings",
        get(get_warnings).put(set_warnings),
    )
}

/// The warnings now (also kept current by `screen.warnings`).
#[utoipa::path(
    get,
    path = "/api/admin/screen-tools/warnings",
    tag = "admin",
    responses((status = 200, body = WarningsPayload))
)]
pub(crate) async fn get_warnings(State(s): State<Arc<Overlays>>) -> Json<WarningsPayload> {
    let (settings, warnings) = s.warnings();
    Json(WarningsPayload::new(&settings, &warnings))
}

#[utoipa::path(
    put,
    path = "/api/admin/screen-tools/warnings",
    tag = "admin",
    request_body = WarningSettingsBody,
    responses(
        (status = 200, body = WarningsPayload),
        (status = 400, description = "`overlays.invalid`: out of range", body = ErrorBody),
        (status = 500, description = "`overlays.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn set_warnings(
    State(s): State<Arc<Overlays>>,
    body: Result<Json<WarningSettingsBody>, JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return error(StatusCode::BAD_REQUEST, "overlays.invalid");
    };
    let settings = WarningSettings {
        chat_overlay: b.chat_overlay,
        twitch: b.twitch,
        mic: b.mic,
        mic_seconds: b.mic_seconds,
        mic_input: b.mic_input,
    };
    match s.set_warning_settings(settings, Timestamp::now()).await {
        Ok((settings, warnings)) => {
            Json(WarningsPayload::new(&settings, &warnings)).into_response()
        }
        Err(OverlayError::Invalid) => error(StatusCode::BAD_REQUEST, "overlays.invalid"),
        Err(OverlayError::Storage(e)) => storage_error(&e),
    }
}
