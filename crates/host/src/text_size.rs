//! The admin UI's text size in percent, kept in `settings.json`. Admin routes; the caller adds the token check.

use std::path::PathBuf;

use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use toolkit_core::settings;

use crate::admin::{ErrorBody, error};

pub(crate) fn routes(data_dir: PathBuf) -> Router {
    Router::new()
        .route("/api/admin/text-size", get(text_size).put(set_text_size))
        .with_state(data_dir)
}

/// The text size in percent; `null` when never changed (100).
#[utoipa::path(
    get,
    path = "/api/admin/text-size",
    tag = "admin",
    responses((status = 200, body = Option<u16>))
)]
pub(crate) async fn text_size(State(data_dir): State<PathBuf>) -> Json<Option<u16>> {
    Json(settings::load(&data_dir).text_size)
}

/// Saves the text size: 90-150 in steps of 10.
#[utoipa::path(
    put,
    path = "/api/admin/text-size",
    tag = "admin",
    request_body = u16,
    responses(
        (status = 204),
        (status = 400, body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
pub(crate) async fn set_text_size(
    State(data_dir): State<PathBuf>,
    body: Result<Json<u16>, JsonRejection>,
) -> Response {
    // Malformed bodies get the same error code as a bad size instead of axum's plain-text rejection.
    let Ok(Json(size)) = body else {
        return error(StatusCode::BAD_REQUEST, "text_size.invalid");
    };
    if !valid(size) {
        return error(StatusCode::BAD_REQUEST, "text_size.invalid");
    }
    if settings::update(&data_dir, |s| s.text_size = Some(size)).is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed");
    }
    StatusCode::NO_CONTENT.into_response()
}

fn valid(size: u16) -> bool {
    (90..=150).contains(&size) && size.is_multiple_of(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_and_steps() {
        assert!(valid(90) && valid(100) && valid(150));
        assert!(!valid(80) && !valid(160) && !valid(105));
    }
}
