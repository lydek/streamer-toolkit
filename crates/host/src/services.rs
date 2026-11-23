//! Whether the broadcaster uses Twitch and OBS at all, and the modules turned off for one not used, kept in
//! `settings.json`. The admin UI decides which modules need which service; the Host checks the format. Admin
//! routes; the caller adds the token check.

use std::{path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use toolkit_core::settings::{self, ExternalService, ServiceUse};

use crate::{
    admin::{AdminState, ErrorBody, error},
    obs::ObsLink,
};

/// The settings folder, and the connections to start or stop when the choice changes.
#[derive(Clone)]
pub(crate) struct ServicesState {
    pub(crate) data_dir: PathBuf,
    pub(crate) admin: Arc<AdminState>,
    pub(crate) obs: Arc<ObsLink>,
}

const MAX_PARKED: usize = 50;
const MAX_ID_LEN: usize = 32;

pub(crate) fn routes(state: ServicesState) -> Router {
    Router::new()
        .route("/api/admin/services", get(services).put(set_services))
        .with_state(state)
}

/// The saved choice; `null` when never changed (both services in use).
#[utoipa::path(
    get,
    path = "/api/admin/services",
    tag = "admin",
    responses((status = 200, body = Option<ServiceUse>))
)]
pub(crate) async fn services(State(state): State<ServicesState>) -> Json<Option<ServiceUse>> {
    Json(settings::load(&state.data_dir).services)
}

/// Saves the choice and starts or stops the Twitch and OBS connections for it; at most 50 parked modules with IDs of
/// 1-32 characters `[a-z0-9-]`, each service listed once.
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
    State(state): State<ServicesState>,
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
    // One choice at a time, saved and applied together; only a service whose use changes is stopped or started
    // again, the other keeps its connection.
    static APPLYING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _one = APPLYING.lock().await;
    let unused = |service| choice.unused.contains(&service);
    let (twitch_off, obs_off) = (
        unused(ExternalService::Twitch),
        unused(ExternalService::Obs),
    );
    let (mut twitch, mut obs) = (false, false);
    if settings::update(&state.data_dir, |s| {
        twitch = s.unused(ExternalService::Twitch) != twitch_off;
        obs = s.unused(ExternalService::Obs) != obs_off;
        s.services = Some(choice);
    })
    .is_err()
    {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed");
    }
    if twitch {
        state.admin.apply_services().await;
    }
    if obs {
        state.obs.restart();
    }
    StatusCode::NO_CONTENT.into_response()
}
