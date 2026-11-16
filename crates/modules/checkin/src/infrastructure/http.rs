//! Admin API for check-ins. The host puts these routes behind the admin token.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::QueryRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use toolkit_core::settings::CheckinSettings;
use utoipa::{IntoParams, ToSchema};

use crate::{
    application::{CheckinService, ConfigError, ManualError, Outcome, settings_of},
    infrastructure::db,
};

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 100;
/// Latest check-ins of the day shown in the drawer.
const RECENT: u32 = 5;

pub(crate) struct HttpState {
    pool: SqlitePool,
    service: Arc<CheckinService>,
}

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

pub fn routes(pool: SqlitePool, service: Arc<CheckinService>) -> Router {
    Router::new()
        .route(
            "/api/admin/checkin/settings",
            get(checkin_settings).put(set_checkin_settings),
        )
        .route("/api/admin/checkin/viewers", get(list_viewers))
        .route("/api/admin/checkin/today", get(today))
        .route(
            "/api/admin/checkin/viewers/{id}/check-ins",
            post(manual_check_in),
        )
        .with_state(Arc::new(HttpState { pool, service }))
}

/// One viewer with the stamp card derived from the total. `user_login` and `user_name` are raw Twitch text: escape
/// them when rendering.
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckinViewer {
    pub id: String,
    pub user_login: String,
    pub user_name: String,
    pub total: u64,
    /// Stamps on the current card, out of `card_slots`.
    pub stamps: u32,
    pub card_slots: u32,
    /// Business day (`YYYY-MM-DD`) of the latest check-in; `null` when there is none.
    pub last_check_in: Option<String>,
}

/// `GET /api/admin/checkin/viewers`: latest check-in first.
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckinViewerPage {
    pub items: Vec<CheckinViewer>,
    /// Viewers matching the search, across all pages.
    pub total: u64,
}

/// Empty values mean no filter.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CheckinViewerQuery {
    /// Part of the login or display name; ASCII letters ignore case.
    search: Option<String>,
    /// Viewers to skip; default 0.
    offset: Option<u32>,
    /// Page size, 1 to 100; default 50.
    limit: Option<u32>,
}

/// Today's check-ins (the business day in force now).
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckinToday {
    /// `YYYY-MM-DD`.
    pub business_day: String,
    pub count: u64,
    /// The latest few, newest first.
    pub recent: Vec<CheckinRecent>,
}

/// `user_name` is raw Twitch text.
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckinRecent {
    pub viewer_id: String,
    pub user_name: String,
    /// RFC 3339, UTC.
    pub occurred_at: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ManualCheckInBody {
    /// A past business day to back-fill (`YYYY-MM-DD`); omitted or `null` registers today.
    #[serde(default)]
    pub date: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ManualCheckInResult {
    CheckedIn,
    /// The viewer already has a check-in on that business day; nothing changed.
    AlreadyCheckedIn,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ManualCheckIn {
    pub result: ManualCheckInResult,
}

fn storage_error(e: impl std::fmt::Display) -> Response {
    tracing::error!(code = "checkin.storage_failed", error = %e, "check-ins not read");
    error(StatusCode::INTERNAL_SERVER_ERROR, "checkin.storage_failed")
}

#[utoipa::path(
    get,
    path = "/api/admin/checkin/viewers",
    tag = "admin",
    params(CheckinViewerQuery),
    responses(
        (status = 200, body = CheckinViewerPage),
        (status = 400, description = "`checkin.invalid_query`: a bad page size", body = ErrorBody),
        (status = 500, description = "`checkin.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn list_viewers(
    State(s): State<Arc<HttpState>>,
    query: Result<Query<CheckinViewerQuery>, QueryRejection>,
) -> Response {
    let Some(Query(q)) = query.ok() else {
        return error(StatusCode::BAD_REQUEST, "checkin.invalid_query");
    };
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return error(StatusCode::BAD_REQUEST, "checkin.invalid_query");
    }
    let search = q
        .search
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty());
    let slots = s.service.config().rules().slots;
    match db::list_viewers(&s.pool, search.as_deref(), q.offset.unwrap_or(0), limit).await {
        Ok((rows, total)) => Json(CheckinViewerPage {
            items: rows
                .into_iter()
                .map(|r| CheckinViewer {
                    stamps: slots.stamps(r.total),
                    card_slots: slots.get(),
                    id: r.id,
                    user_login: r.user_login,
                    user_name: r.user_name,
                    total: r.total,
                    last_check_in: r.last_day,
                })
                .collect(),
            total,
        })
        .into_response(),
        Err(e) => storage_error(e),
    }
}

#[utoipa::path(
    get,
    path = "/api/admin/checkin/today",
    tag = "admin",
    responses(
        (status = 200, body = CheckinToday),
        (status = 500, description = "`checkin.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn today(State(s): State<Arc<HttpState>>) -> Response {
    let day = s.service.config().rules().day.day_of(Utc::now());
    match db::day_summary(&s.pool, day, RECENT).await {
        Ok((count, recent)) => Json(CheckinToday {
            business_day: day.to_string(),
            count,
            recent: recent
                .into_iter()
                .map(|(viewer_id, user_name, occurred_at)| CheckinRecent {
                    viewer_id,
                    user_name,
                    occurred_at,
                })
                .collect(),
        })
        .into_response(),
        Err(e) => storage_error(e),
    }
}

#[utoipa::path(
    post,
    path = "/api/admin/checkin/viewers/{id}/check-ins",
    tag = "admin",
    params(("id" = String, Path, description = "The viewer's ID")),
    request_body = ManualCheckInBody,
    responses(
        (status = 200, description = "Recorded, or already checked in that day", body = ManualCheckIn),
        (status = 400, description = "`checkin.invalid_date`: not a date, or a future business day", body = ErrorBody),
        (status = 404, description = "`checkin.viewer_not_found`", body = ErrorBody),
        (status = 500, description = "`checkin.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn manual_check_in(
    State(s): State<Arc<HttpState>>,
    Path(id): Path<String>,
    Json(body): Json<ManualCheckInBody>,
) -> Response {
    let day = match body
        .date
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        None => None,
        Some(text) => match NaiveDate::parse_from_str(text, "%Y-%m-%d") {
            Ok(day) => Some(day),
            Err(_) => return error(StatusCode::BAD_REQUEST, "checkin.invalid_date"),
        },
    };
    let result = match s.service.check_in_manually(&id, day).await {
        Ok(Outcome::CheckedIn { .. }) => ManualCheckInResult::CheckedIn,
        Ok(Outcome::Repeat) => ManualCheckInResult::AlreadyCheckedIn,
        Err(e @ ManualError::ViewerNotFound) => {
            return error(StatusCode::NOT_FOUND, &e.to_string());
        }
        Err(e @ ManualError::FutureDay) => return error(StatusCode::BAD_REQUEST, &e.to_string()),
        Err(ManualError::Storage(e)) => return storage_error(e),
    };
    Json(ManualCheckIn { result }).into_response()
}

#[utoipa::path(
    get,
    path = "/api/admin/checkin/settings",
    tag = "admin",
    responses((status = 200, body = CheckinSettingsBody))
)]
pub(crate) async fn checkin_settings(State(s): State<Arc<HttpState>>) -> Json<CheckinSettingsBody> {
    Json(settings_of(&s.service.config().rules()).into())
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
    State(s): State<Arc<HttpState>>,
    Json(body): Json<CheckinSettingsBody>,
) -> Response {
    let next = CheckinSettings {
        command: body.command,
        time_zone: body.time_zone.trim().to_owned(),
        reset_hour: body.reset_hour,
        card_slots: body.card_slots,
        reward_id: body.reward_id.filter(|id| !id.trim().is_empty()),
    };
    match s.service.config().update(&next) {
        Ok(rules) => Json(CheckinSettingsBody::from(settings_of(&rules))).into_response(),
        Err(e @ ConfigError::Invalid(_)) => error(StatusCode::BAD_REQUEST, &e.to_string()),
        Err(e @ ConfigError::SaveFailed(_)) => {
            error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
        }
    }
}
