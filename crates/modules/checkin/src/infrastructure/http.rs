//! Admin API for check-ins. The host puts these routes behind the admin token.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Path, Query, State, rejection::JsonRejection, rejection::QueryRejection,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use toolkit_core::{
    commands::{Allowed, CommandError, CommandTakenBody},
    settings::CheckinSettings,
};
use utoipa::{IntoParams, ToSchema};

use crate::{
    application::{self, CheckinService, ConfigError, Deducted, ManualError, Outcome, settings_of},
    domain::{Deduction, ImportEntry, ImportedViewer, Rules, SkipReason},
    infrastructure::db,
};
use toolkit_core::time::Timestamp;

/// Largest import file accepted.
pub const IMPORT_MAX_BYTES: usize = 5 * 1024 * 1024;

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 100;
/// Latest check-ins of the day shown in the drawer.
const RECENT: u32 = 5;
/// How many deductions the change log lists.
const DEDUCTIONS_LISTED: u32 = 100;

pub(crate) struct HttpState {
    pool: SqlitePool,
    service: Arc<CheckinService>,
}

/// The check-in settings. `command` is stored without its `!`; a leading one is accepted and dropped.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CheckinSettingsBody {
    /// One word, at most 25 characters; no other feature's command may have it (ignoring case).
    pub command: String,
    /// Who may check in with the command; the broadcaster always may.
    pub allowed: Allowed,
    /// IANA time zone, e.g. `Asia/Taipei`.
    pub time_zone: String,
    /// Local hour a new business day starts, 0–23.
    pub reset_hour: u32,
    /// Stamp card slots, 1–100.
    pub card_slots: u32,
    /// Twitch ID of the channel points reward that checks in; `null`: chat only.
    pub reward_id: Option<String>,
}

impl CheckinSettingsBody {
    fn of(rules: &Rules) -> Self {
        let s = settings_of(rules);
        Self {
            allowed: rules.allowed,
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

pub(crate) fn error(status: StatusCode, code: &str) -> Response {
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
        .route(
            "/api/admin/checkin/viewers/{id}/deductions",
            post(deduct_stamps),
        )
        .route("/api/admin/checkin/deductions", get(list_deductions))
        .route("/api/admin/checkin/viewers/{id}/card", post(push_card))
        .route("/api/admin/checkin/card/hide", post(hide_card))
        .route(
            "/api/admin/checkin/import/preview",
            post(preview_import).layer(DefaultBodyLimit::max(IMPORT_MAX_BYTES)),
        )
        .route(
            "/api/admin/checkin/import",
            post(apply_import).layer(DefaultBodyLimit::max(IMPORT_MAX_BYTES)),
        )
        .with_state(Arc::new(HttpState {
            pool,
            service: service.clone(),
        }))
        .merge(super::card_style::routes().with_state(service))
}

/// One viewer with the stamp card derived from the total. `user_login` and `user_name` are raw Twitch text: escape
/// them when rendering.
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckinViewer {
    pub id: String,
    pub user_login: String,
    pub user_name: String,
    /// Lifetime check-ins; deductions never lower it.
    pub total: u64,
    /// Stamps that can be spent: `total` minus what was deducted, never below 0.
    pub available: u64,
    /// Stamps on the current card, out of `card_slots`; from `total`.
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

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeductBody {
    /// 1 to 1,000,000, and at most what is available.
    pub amount: u32,
    /// Optional, at most 100 characters.
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeductResult {
    /// Stamps still available after it.
    pub available: u64,
}

/// More than is available; nothing was deducted.
#[derive(Debug, Serialize, ToSchema)]
pub struct NotEnoughStamps {
    /// `checkin.not_enough_stamps`.
    pub code: String,
    pub available: u64,
}

/// One deduction in the change log. `user_name` and `reason` are raw text: escape them when rendering.
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckinDeduction {
    pub id: String,
    pub viewer_id: String,
    pub user_name: String,
    pub amount: u32,
    pub reason: String,
    /// RFC 3339, UTC.
    pub deducted_at: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImportSkipReason {
    /// The key is not a numeric Twitch user ID.
    InvalidId,
    /// `isOwner` is `true`.
    Owner,
    /// `excluded` is `true`.
    Excluded,
    /// `totalCheckIns` is missing, not an integer, negative, or above 1,000,000,000.
    InvalidTotal,
    /// `lastCheckInDate` is not `YYYY-MM-DD`.
    InvalidDate,
}

impl From<SkipReason> for ImportSkipReason {
    fn from(r: SkipReason) -> Self {
        match r {
            SkipReason::InvalidId => Self::InvalidId,
            SkipReason::Owner => Self::Owner,
            SkipReason::Excluded => Self::Excluded,
            SkipReason::InvalidTotal => Self::InvalidTotal,
            SkipReason::InvalidDate => Self::InvalidDate,
        }
    }
}

/// An entry left out. `key` is the file's own text (at most 64 characters): escape it when rendering.
#[derive(Debug, Serialize, ToSchema)]
pub struct ImportSkip {
    pub key: String,
    pub reason: ImportSkipReason,
}

/// What an import adds, updates (replacing the imported baseline of known viewers) and skips.
#[derive(Debug, Serialize, ToSchema)]
pub struct ImportSummary {
    pub added: u64,
    pub updated: u64,
    pub skipped: Vec<ImportSkip>,
}

/// Reads the import format (docs/modules/checkin.md): an object keyed by Twitch user ID. Unknown fields and
/// `stamps` are ignored.
fn read_import(body: &serde_json::Value) -> Option<(Vec<ImportedViewer>, Vec<ImportSkip>)> {
    let entries = body.as_object()?;
    let mut viewers = Vec::new();
    let mut skipped = Vec::new();
    for (key, value) in entries {
        let field = |name: &str| value.get(name);
        let text = |name: &str| field(name).and_then(serde_json::Value::as_str);
        let flag = |name: &str| field(name).and_then(serde_json::Value::as_bool) == Some(true);
        let entry = ImportEntry {
            key,
            total: field("totalCheckIns").and_then(serde_json::Value::as_i64),
            is_owner: flag("isOwner"),
            excluded: flag("excluded"),
            // A present but non-text date is invalid, not missing.
            last_date: match field("lastCheckInDate") {
                None | Some(serde_json::Value::Null) => None,
                Some(v) => Some(v.as_str().unwrap_or("")),
            },
            name: text("lastName").or_else(|| text("name")),
            profile_image: text("profileImage"),
        };
        match ImportedViewer::check(&entry) {
            Ok(v) => viewers.push(v),
            Err(reason) => skipped.push(ImportSkip {
                key: key.chars().take(64).collect(),
                reason: reason.into(),
            }),
        }
    }
    Some((viewers, skipped))
}

async fn import(
    s: &HttpState,
    body: Result<Json<serde_json::Value>, JsonRejection>,
    apply: bool,
) -> Response {
    let body = match body {
        Ok(Json(v)) => v,
        Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return error(StatusCode::PAYLOAD_TOO_LARGE, "checkin.import_too_large");
        }
        Err(_) => return error(StatusCode::BAD_REQUEST, "checkin.import_invalid"),
    };
    let Some((viewers, skipped)) = read_import(&body) else {
        return error(StatusCode::BAD_REQUEST, "checkin.import_invalid");
    };
    let repo = db::SqliteCheckins::new(s.pool.clone());
    match application::import(&repo, &viewers, apply, Timestamp::now()).await {
        Ok(counts) => Json(ImportSummary {
            added: counts.added,
            updated: counts.updated,
            skipped,
        })
        .into_response(),
        Err(e) => storage_error(e),
    }
}

/// What importing this file would change; nothing is written. The admin UI posts the chosen file's content.
#[utoipa::path(
    post,
    path = "/api/admin/checkin/import/preview",
    tag = "admin",
    request_body(content = Object, description = "The import file: an object keyed by Twitch user ID (at most 5 MB)"),
    responses(
        (status = 200, body = ImportSummary),
        (status = 400, description = "`checkin.import_invalid`: not JSON, or not an object", body = ErrorBody),
        (status = 413, description = "`checkin.import_too_large`: larger than 5 MB", body = ErrorBody),
        (status = 500, description = "`checkin.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn preview_import(
    State(s): State<Arc<HttpState>>,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> Response {
    import(&s, body, false).await
}

/// Imports the file in one transaction; the same file imported again gives the same totals.
#[utoipa::path(
    post,
    path = "/api/admin/checkin/import",
    tag = "admin",
    request_body(content = Object, description = "The import file: an object keyed by Twitch user ID (at most 5 MB)"),
    responses(
        (status = 200, body = ImportSummary),
        (status = 400, description = "`checkin.import_invalid`: not JSON, or not an object", body = ErrorBody),
        (status = 413, description = "`checkin.import_too_large`: larger than 5 MB", body = ErrorBody),
        (status = 500, description = "`checkin.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn apply_import(
    State(s): State<Arc<HttpState>>,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> Response {
    import(&s, body, true).await
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
                    available: crate::domain::available(r.total, r.deducted),
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
    let day = s.service.config().rules().day.day_of(Timestamp::now());
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
        Some(text) => match jiff::civil::Date::strptime("%Y-%m-%d", text) {
            Ok(day) => Some(day),
            Err(_) => return error(StatusCode::BAD_REQUEST, "checkin.invalid_date"),
        },
    };
    let result = match s.service.check_in_manually(&id, day).await {
        Ok(Outcome::CheckedIn { .. }) => ManualCheckInResult::CheckedIn,
        Ok(Outcome::Repeat | Outcome::Redelivered) => ManualCheckInResult::AlreadyCheckedIn,
        Err(e @ ManualError::ViewerNotFound) => {
            return error(StatusCode::NOT_FOUND, &e.to_string());
        }
        Err(e @ ManualError::FutureDay) => return error(StatusCode::BAD_REQUEST, &e.to_string()),
        Err(ManualError::Storage(e)) => return storage_error(e),
    };
    Json(ManualCheckIn { result }).into_response()
}

/// Spends a viewer's stamps; the total and the card stay as they are.
#[utoipa::path(
    post,
    path = "/api/admin/checkin/viewers/{id}/deductions",
    tag = "admin",
    params(("id" = String, Path, description = "The viewer's internal ID")),
    request_body = DeductBody,
    responses(
        (status = 200, body = DeductResult),
        (status = 400, description = "`checkin.invalid_deduction`: an amount out of range or a reason too long", body = ErrorBody),
        (status = 404, description = "`checkin.viewer_not_found`", body = ErrorBody),
        (status = 409, description = "`checkin.not_enough_stamps`", body = NotEnoughStamps),
        (status = 500, description = "`checkin.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn deduct_stamps(
    State(s): State<Arc<HttpState>>,
    Path(id): Path<String>,
    body: Result<Json<DeductBody>, JsonRejection>,
) -> Response {
    let Some(deduction) = body
        .ok()
        .and_then(|Json(b)| Deduction::new(b.amount, &b.reason).ok())
    else {
        return error(StatusCode::BAD_REQUEST, "checkin.invalid_deduction");
    };
    match s.service.deduct(&id, deduction).await {
        Ok(Deducted::Done { available }) => Json(DeductResult { available }).into_response(),
        Ok(Deducted::NotEnough { available }) => (
            StatusCode::CONFLICT,
            Json(NotEnoughStamps {
                code: "checkin.not_enough_stamps".into(),
                available,
            }),
        )
            .into_response(),
        Ok(Deducted::UnknownViewer) => error(StatusCode::NOT_FOUND, "checkin.viewer_not_found"),
        Err(e) => storage_error(e),
    }
}

/// Puts the viewer's stamp card on the screen through the presentation queue; no check-in is recorded.
#[utoipa::path(
    post,
    path = "/api/admin/checkin/viewers/{id}/card",
    tag = "admin",
    params(("id" = String, Path, description = "The viewer's internal ID")),
    responses(
        (status = 204),
        (status = 404, description = "`checkin.viewer_not_found`", body = ErrorBody),
        (status = 500, description = "`checkin.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn push_card(State(s): State<Arc<HttpState>>, Path(id): Path<String>) -> Response {
    match s.service.push_card(&id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => error(StatusCode::NOT_FOUND, "checkin.viewer_not_found"),
        Err(e) => storage_error(e),
    }
}

/// 隱藏畫面卡片: takes the stamp card on the screen away at once.
#[utoipa::path(
    post,
    path = "/api/admin/checkin/card/hide",
    tag = "admin",
    responses((status = 204))
)]
pub(crate) async fn hide_card(State(s): State<Arc<HttpState>>) -> StatusCode {
    s.service.hide_card();
    StatusCode::NO_CONTENT
}

/// The change log: the latest 100 deductions, newest first.
#[utoipa::path(
    get,
    path = "/api/admin/checkin/deductions",
    tag = "admin",
    responses(
        (status = 200, body = Vec<CheckinDeduction>),
        (status = 500, description = "`checkin.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn list_deductions(State(s): State<Arc<HttpState>>) -> Response {
    match db::deductions(&s.pool, DEDUCTIONS_LISTED).await {
        Ok(rows) => Json(
            rows.into_iter()
                .map(|d| CheckinDeduction {
                    id: d.id,
                    viewer_id: d.viewer_id,
                    user_name: d.user_name,
                    amount: d.amount,
                    reason: d.reason,
                    deducted_at: d.deducted_at,
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => storage_error(e),
    }
}

#[utoipa::path(
    get,
    path = "/api/admin/checkin/settings",
    tag = "admin",
    responses((status = 200, body = CheckinSettingsBody))
)]
pub(crate) async fn checkin_settings(State(s): State<Arc<HttpState>>) -> Json<CheckinSettingsBody> {
    Json(CheckinSettingsBody::of(&s.service.config().rules()))
}

#[utoipa::path(
    put,
    path = "/api/admin/checkin/settings",
    tag = "admin",
    request_body = CheckinSettingsBody,
    responses(
        (status = 200, description = "The settings as saved", body = CheckinSettingsBody),
        (status = 400, description = "`checkin.invalid_settings`", body = ErrorBody),
        (status = 409, description = "`commands.taken`: another feature's command has the name", body = CommandTakenBody),
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
        // Kept as saved: the card style has its own route.
        card: Default::default(),
    };
    match s.service.config().update(&next, body.allowed) {
        Ok(rules) => Json(CheckinSettingsBody::of(&rules)).into_response(),
        Err(e @ ConfigError::Invalid(_)) => error(StatusCode::BAD_REQUEST, &e.to_string()),
        Err(ConfigError::Command(CommandError::Taken { module, label })) => (
            StatusCode::CONFLICT,
            Json(CommandTakenBody::new(module, label)),
        )
            .into_response(),
        Err(ConfigError::Command(e @ CommandError::SaveFailed(_))) => {
            error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
        }
    }
}
