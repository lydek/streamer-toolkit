//! The admin UI's docked chat panel (open, width), kept in `settings.json`. Admin routes; the caller adds the
//! token check.

use std::path::PathBuf;

use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use toolkit_core::settings::{self, ChatDock};

use crate::admin::{ErrorBody, error};

/// CSS pixels the admin UI lets the panel be resized to.
const WIDTHS: std::ops::RangeInclusive<u16> = 280..=560;

pub(crate) fn routes(data_dir: PathBuf) -> Router {
    Router::new()
        .route("/api/admin/chat-dock", get(chat_dock).put(set_chat_dock))
        .with_state(data_dir)
}

/// The panel's state; `null` when never changed (the admin UI shows its defaults).
#[utoipa::path(
    get,
    path = "/api/admin/chat-dock",
    tag = "admin",
    responses((status = 200, body = Option<ChatDock>))
)]
pub(crate) async fn chat_dock(State(data_dir): State<PathBuf>) -> Json<Option<ChatDock>> {
    Json(settings::load(&data_dir).chat_dock)
}

/// Saves the panel's state; `width` is 280-560.
#[utoipa::path(
    put,
    path = "/api/admin/chat-dock",
    tag = "admin",
    request_body = ChatDock,
    responses(
        (status = 204),
        (status = 400, body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
pub(crate) async fn set_chat_dock(
    State(data_dir): State<PathBuf>,
    body: Result<Json<ChatDock>, JsonRejection>,
) -> Response {
    // Malformed bodies get the same error code as a bad width instead of axum's plain-text rejection.
    let Ok(Json(dock)) = body else {
        return error(StatusCode::BAD_REQUEST, "chat_dock.invalid");
    };
    if !WIDTHS.contains(&dock.width) {
        return error(StatusCode::BAD_REQUEST, "chat_dock.invalid");
    }
    if settings::update(&data_dir, |s| s.chat_dock = Some(dock)).is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed");
    }
    StatusCode::NO_CONTENT.into_response()
}
