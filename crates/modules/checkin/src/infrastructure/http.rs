//! Admin API for check-ins. The host puts these routes behind the admin token.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use toolkit_core::settings::CheckinSettings;
use utoipa::ToSchema;

use crate::application::{CheckinService, ConfigError, settings_of};

/// The check-in settings. `command` is stored without its `!`; a leading one is accepted and dropped.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CheckinSettingsBody {
    /// One word, at most 25 characters.
    pub command: String,
    /// IANA time zone, e.g. `Asia/Taipei`.
    pub time_zone: String,
    /// Local hour a new business day starts, 0–23.
    pub reset_hour: u32,
    /// Stamp card slots, 1–100.
    pub card_slots: u32,
    /// Twitch ID of the channel points reward that checks in; `null`: chat only.
    pub reward_id: Option<String>,
}

impl From<CheckinSettings> for CheckinSettingsBody {
    fn from(s: CheckinSettings) -> Self {
        Self {
            command: s.command,
            time_zone: s.time_zone,
            reset_hour: s.reset_hour,
            card_slots: s.card_slots,
            reward_id: s.reward_id,
        }
    }
}

/// Same shape as the host's admin error body: a stable `<area>.<reason>` code.
#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    code: String,
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(ErrorBody { code: code.into() })).into_response()
}

pub fn routes(service: Arc<CheckinService>) -> Router {
    Router::new()
        .route(
            "/api/admin/checkin/settings",
            get(checkin_settings).put(set_checkin_settings),
        )
        .with_state(service)
}

#[utoipa::path(
    get,
    path = "/api/admin/checkin/settings",
    tag = "admin",
    responses((status = 200, body = CheckinSettingsBody))
)]
pub(crate) async fn checkin_settings(
    State(s): State<Arc<CheckinService>>,
) -> Json<CheckinSettingsBody> {
    Json(settings_of(&s.config().rules()).into())
}

#[utoipa::path(
    put,
    path = "/api/admin/checkin/settings",
    tag = "admin",
    request_body = CheckinSettingsBody,
    responses(
        (status = 200, description = "The settings as saved", body = CheckinSettingsBody),
        (status = 400, description = "`checkin.invalid_settings`", body = ErrorBody),
        (status = 500, description = "`settings.save_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn set_checkin_settings(
    State(s): State<Arc<CheckinService>>,
    Json(body): Json<CheckinSettingsBody>,
) -> Response {
    let next = CheckinSettings {
        command: body.command,
        time_zone: body.time_zone.trim().to_owned(),
        reset_hour: body.reset_hour,
        card_slots: body.card_slots,
        reward_id: body.reward_id.filter(|id| !id.trim().is_empty()),
    };
    match s.config().update(&next) {
        Ok(rules) => Json(CheckinSettingsBody::from(settings_of(&rules))).into_response(),
        Err(e @ ConfigError::Invalid(_)) => error(StatusCode::BAD_REQUEST, &e.to_string()),
        Err(e @ ConfigError::SaveFailed(_)) => {
            error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
        }
    }
}
