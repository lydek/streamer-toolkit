//! Admin API: 最近事件 (the latest alerts and what became of each) and replaying one. The caller adds the token
//! check.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sound::SoundService;
use toolkit_core::{events::Source, presentation::Outcome};
use utoipa::ToSchema;

use super::{AlertPayload, alert_payload};
use crate::application::AlertsService;

#[derive(Clone)]
pub(crate) struct Http {
    service: Arc<AlertsService>,
    sounds: Arc<SoundService>,
}

pub fn routes(service: Arc<AlertsService>, sounds: Arc<SoundService>) -> Router {
    Router::new()
        .route("/api/admin/alerts/recent", get(recent))
        .route("/api/admin/alerts/recent/{id}/replay", post(replay))
        .with_state(Http { service, sounds })
}

#[derive(Serialize, Deserialize, ToSchema)]
pub(crate) struct ErrorBody {
    code: String,
}

/// One alert in 最近事件.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecentAlertBody {
    pub id: String,
    pub occurred_at: String,
    pub source: Source,
    /// What became of it; `null` while it waits or plays.
    #[schema(required)]
    pub outcome: Option<Outcome>,
    /// As the overlay gets it now (the sound follows the current settings).
    pub payload: AlertPayload,
}

/// The latest alerts this launch, newest first (at most 50), with what became of each.
#[utoipa::path(
    get,
    path = "/api/admin/alerts/recent",
    tag = "admin",
    responses((status = 200, body = Vec<RecentAlertBody>))
)]
pub(crate) async fn recent(State(h): State<Http>) -> Json<Vec<RecentAlertBody>> {
    Json(
        h.service
            .recent()
            .into_iter()
            .map(|r| RecentAlertBody {
                id: r.alert.id.clone(),
                occurred_at: r.alert.occurred_at.clone(),
                source: r.source,
                outcome: r.outcome,
                payload: alert_payload(r.alert, r.show, &h.sounds),
            })
            .collect(),
    )
}

/// Queues an alert from 最近事件 again with the current template and sounds; nothing is recorded again.
#[utoipa::path(
    post,
    path = "/api/admin/alerts/recent/{id}/replay",
    tag = "admin",
    params(("id" = String, Path, description = "The alert's ID from 最近事件")),
    responses((status = 204), (status = 404, body = ErrorBody))
)]
pub(crate) async fn replay(State(h): State<Http>, Path(id): Path<String>) -> Response {
    if h.service.replay(&id) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                code: "alerts.not_found".into(),
            }),
        )
            .into_response()
    }
}
