//! How the admin UI's 下播後 home shows its numbers (show, mask, hide; numbers revealed one by one), kept in
//! `settings.json`. Admin routes; the caller adds the token check.

use std::path::PathBuf;

use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use toolkit_core::settings::{self, AfterNumbers};

use crate::admin::{ErrorBody, error};

const MAX_REVEALED: usize = 50;
const MAX_ID_LEN: usize = 32;

pub(crate) fn routes(data_dir: PathBuf) -> Router {
    Router::new()
        .route(
            "/api/admin/after-numbers",
            get(after_numbers).put(set_after_numbers),
        )
        .with_state(data_dir)
}

/// The saved choice; `null` when never changed (numbers show).
#[utoipa::path(
    get,
    path = "/api/admin/after-numbers",
    tag = "admin",
    responses((status = 200, body = Option<AfterNumbers>))
)]
pub(crate) async fn after_numbers(State(data_dir): State<PathBuf>) -> Json<Option<AfterNumbers>> {
    Json(settings::load(&data_dir).after_numbers)
}

/// Saves the choice; at most 50 revealed IDs of 1-32 characters `[a-z0-9.-]`.
#[utoipa::path(
    put,
    path = "/api/admin/after-numbers",
    tag = "admin",
    request_body = AfterNumbers,
    responses(
        (status = 204),
        (status = 400, body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
pub(crate) async fn set_after_numbers(
    State(data_dir): State<PathBuf>,
    body: Result<Json<AfterNumbers>, JsonRejection>,
) -> Response {
    let Ok(Json(numbers)) = body else {
        return error(StatusCode::BAD_REQUEST, "after_numbers.invalid");
    };
    let valid_id = |id: &String| {
        (1..=MAX_ID_LEN).contains(&id.len())
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
    };
    if numbers.revealed.len() > MAX_REVEALED || !numbers.revealed.iter().all(valid_id) {
        return error(StatusCode::BAD_REQUEST, "after_numbers.invalid");
    }
    if settings::update(&data_dir, |s| s.after_numbers = Some(numbers)).is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed");
    }
    StatusCode::NO_CONTENT.into_response()
}
