//! Whether the broadcaster uses Twitch and OBS at all, and the modules turned off for one not used, kept in
//! `settings.json`. The admin UI decides which modules need which service; the Host checks the format. Admin
//! routes; the caller adds the token check.

use std::path::PathBuf;

use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use toolkit_core::settings::{self, ServiceUse};

use crate::admin::{ErrorBody, error};

const MAX_PARKED: usize = 50;
const MAX_ID_LEN: usize = 32;

pub(crate) fn routes(data_dir: PathBuf) -> Router {
    Router::new()
        .route("/api/admin/services", get(services).put(set_services))
        .with_state(data_dir)
}

/// The saved choice; `null` when never changed (both services in use).
#[utoipa::path(
    get,
    path = "/api/admin/services",
    tag = "admin",
    responses((status = 200, body = Option<ServiceUse>))
)]
pub(crate) async fn services(State(data_dir): State<PathBuf>) -> Json<Option<ServiceUse>> {
    Json(settings::load(&data_dir).services)
}

/// Saves the choice; at most 50 parked modules with IDs of 1-32 characters `[a-z0-9-]`, each service listed once.
#[utoipa::path(
    put,
    path = "/api/admin/services",
    tag = "admin",
    request_body = ServiceUse,
    responses(
        (status = 204),
        (status = 400, body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
pub(crate) async fn set_services(
    State(data_dir): State<PathBuf>,
    body: Result<Json<ServiceUse>, JsonRejection>,
) -> Response {
    let Ok(Json(choice)) = body else {
        return error(StatusCode::BAD_REQUEST, "services.invalid");
    };
    let valid_id = |id: &str| {
        (1..=MAX_ID_LEN).contains(&id.len())
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    let repeated = choice
        .unused
        .iter()
        .enumerate()
        .any(|(i, s)| choice.unused[..i].contains(s));
    if repeated
        || choice.parked.len() > MAX_PARKED
        || !choice.parked.iter().all(|p| valid_id(&p.id))
    {
        return error(StatusCode::BAD_REQUEST, "services.invalid");
    }
    if settings::update(&data_dir, |s| s.services = Some(choice)).is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed");
    }
    StatusCode::NO_CONTENT.into_response()
}
