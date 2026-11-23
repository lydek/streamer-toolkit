//! Which OBS input the 音訊混音 page treats as the mic, kept in `settings.json`. Admin routes; the caller adds the
//! token check.

use std::path::PathBuf;

use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use toolkit_core::settings;
use utoipa::ToSchema;

use crate::admin::{ErrorBody, error};

/// OBS caps nothing, but a name this long is not a real input.
const MAX_NAME_LEN: usize = 256;

pub(crate) fn routes(data_dir: PathBuf) -> Router {
    Router::new()
        .route("/api/admin/mixer/mic", get(mic).put(set_mic))
        .with_state(data_dir)
}

#[derive(Serialize, Deserialize, ToSchema)]
pub(crate) struct MixerMic {
    /// An OBS input's name; `null`: not chosen.
    input: Option<String>,
}

/// The input chosen as the mic; `null` when none is.
#[utoipa::path(
    get,
    path = "/api/admin/mixer/mic",
    tag = "admin",
    responses((status = 200, body = MixerMic))
)]
pub(crate) async fn mic(State(data_dir): State<PathBuf>) -> Json<MixerMic> {
    Json(MixerMic {
        input: settings::load(&data_dir).mixer_mic,
    })
}

/// Chooses the mic (an input name of 1-256 characters without control characters), or clears it with `null`. The
/// input is not checked against OBS, which may be closed; the mixer says so when it is missing.
#[utoipa::path(
    put,
    path = "/api/admin/mixer/mic",
    tag = "admin",
    request_body = MixerMic,
    responses(
        (status = 204),
        (status = 400, body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
pub(crate) async fn set_mic(
    State(data_dir): State<PathBuf>,
    body: Result<Json<MixerMic>, JsonRejection>,
) -> Response {
    let Ok(Json(MixerMic { input })) = body else {
        return error(StatusCode::BAD_REQUEST, "mixer.invalid_mic");
    };
    if input.as_ref().is_some_and(|name| {
        name.is_empty() || name.chars().count() > MAX_NAME_LEN || name.chars().any(char::is_control)
    }) {
        return error(StatusCode::BAD_REQUEST, "mixer.invalid_mic");
    }
    if settings::update(&data_dir, |s| s.mixer_mic = input).is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed");
    }
    StatusCode::NO_CONTENT.into_response()
}
