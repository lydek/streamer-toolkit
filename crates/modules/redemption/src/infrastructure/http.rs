//! Admin API for browsing records. The host puts these routes behind the admin token.

use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use utoipa::{IntoParams, ToSchema};

use std::sync::Arc;

use toolkit_core::presentation::{PlayResult, PlayedKind};

use crate::{
    application::{ListFilter, RedemptionService},
    domain::{Redemption, Status},
    infrastructure::db,
};

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RedemptionStatusName {
    Unfulfilled,
    Fulfilled,
    /// The points were refunded.
    Canceled,
    /// Twitch sent a status this version does not know.
    Unknown,
}

impl From<Status> for RedemptionStatusName {
    fn from(s: Status) -> Self {
        match s {
            Status::Unfulfilled => Self::Unfulfilled,
            Status::Fulfilled => Self::Fulfilled,
            Status::Canceled => Self::Canceled,
            Status::Unknown => Self::Unknown,
        }
    }
}

impl From<RedemptionStatusName> for Status {
    fn from(s: RedemptionStatusName) -> Self {
        match s {
            RedemptionStatusName::Unfulfilled => Self::Unfulfilled,
            RedemptionStatusName::Fulfilled => Self::Fulfilled,
            RedemptionStatusName::Canceled => Self::Canceled,
            RedemptionStatusName::Unknown => Self::Unknown,
        }
    }
}

/// One redemption record. `user_name`, `reward_title` and `user_input` are raw viewer or Twitch text: escape them
/// when rendering.
#[derive(Debug, Serialize, ToSchema)]
pub struct RedemptionRecord {
    pub id: String,
    pub user_login: String,
    pub user_name: String,
    pub reward_title: String,
    pub reward_cost: u32,
    /// Empty when the reward asks for nothing.
    pub user_input: String,
    pub status: RedemptionStatusName,
    /// RFC 3339, UTC.
    pub redeemed_at: String,
    /// RFC 3339, UTC: when the record was created or last changed status.
    pub updated_at: String,
    /// What the redemption played, in the order each ended: empty when it played nothing, `null` for a record
    /// from before this was kept.
    #[schema(required)]
    pub played: Option<Vec<PlayedEntry>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlayedEntry {
    pub what: PlayedKind,
    pub result: PlayResult,
}

impl From<Redemption> for RedemptionRecord {
    fn from(r: Redemption) -> Self {
        let time = |t: DateTime<Utc>| t.to_rfc3339_opts(SecondsFormat::Millis, true);
        Self {
            id: r.id.0.to_string(),
            user_login: r.viewer.login,
            user_name: r.viewer.name,
            reward_title: r.reward.title,
            reward_cost: r.reward.cost,
            user_input: r.user_input,
            status: r.status.into(),
            redeemed_at: time(r.redeemed_at),
            updated_at: time(r.updated_at),
            played: None,
        }
    }
}

/// `GET /api/admin/redemptions`: one page, newest first.
#[derive(Debug, Serialize, ToSchema)]
pub struct RedemptionPage {
    pub items: Vec<RedemptionRecord>,
    /// Records matching the filters, across all pages.
    pub total: u64,
    /// Redemptions that could not be stored since the app started, even after retries; they are not in the list.
    pub failed_writes: u64,
}

/// Empty values mean no filter.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RedemptionQuery {
    /// RFC 3339; records redeemed at or after it.
    from: Option<String>,
    /// RFC 3339; records redeemed before it.
    to: Option<String>,
    /// Part of the viewer's login or display name; ASCII letters ignore case.
    viewer: Option<String>,
    /// Part of the reward name; ASCII letters ignore case.
    reward: Option<String>,
    status: Option<RedemptionStatusName>,
    /// Records to skip; default 0.
    offset: Option<u32>,
    /// Page size, 1 to 100; default 50.
    limit: Option<u32>,
}

/// Same shape as the host's admin error body: a stable `<area>.<reason>` code.
#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    code: String,
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(ErrorBody { code: code.into() })).into_response()
}

#[derive(Clone)]
pub(crate) struct Http {
    pool: SqlitePool,
    service: Arc<RedemptionService>,
}

pub fn routes(pool: SqlitePool, service: Arc<RedemptionService>) -> Router {
    Router::new()
        .route("/api/admin/redemptions", get(list_redemptions))
        .with_state(Http { pool, service })
}

fn filter(q: RedemptionQuery) -> Result<ListFilter, ()> {
    let text = |v: Option<String>| v.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    let time = |v: Option<String>| -> Result<Option<DateTime<Utc>>, ()> {
        text(v)
            .map(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|t| t.to_utc())
                    .map_err(|_| ())
            })
            .transpose()
    };
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(());
    }
    Ok(ListFilter {
        from: time(q.from)?,
        to: time(q.to)?,
        viewer: text(q.viewer),
        reward: text(q.reward),
        status: q.status.map(Status::from),
        offset: q.offset.unwrap_or(0),
        limit,
    })
}

#[utoipa::path(
    get,
    path = "/api/admin/redemptions",
    tag = "admin",
    params(RedemptionQuery),
    responses(
        (status = 200, body = RedemptionPage),
        (status = 400, description = "`redemption.invalid_filter`: a bad time, status or page size", body = ErrorBody),
        (status = 500, description = "`redemption.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn list_redemptions(
    State(Http { pool, service }): State<Http>,
    query: Result<Query<RedemptionQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Some(filter) = query.ok().and_then(|Query(q)| filter(q).ok()) else {
        return error(StatusCode::BAD_REQUEST, "redemption.invalid_filter");
    };
    let listed = match db::list(&pool, &filter).await {
        Ok((items, total)) => {
            let ids: Vec<String> = items.iter().map(|r| r.twitch_id.0.clone()).collect();
            db::played(&pool, &ids)
                .await
                .map(|played| (items, total, played))
        }
        Err(e) => Err(e),
    };
    match listed {
        Ok((items, total, mut played)) => Json(RedemptionPage {
            items: items
                .into_iter()
                .map(|r| {
                    let shown = played.remove(&r.twitch_id.0).flatten();
                    RedemptionRecord {
                        played: shown.map(|list| {
                            list.into_iter()
                                .map(|(what, result)| PlayedEntry { what, result })
                                .collect()
                        }),
                        ..RedemptionRecord::from(r)
                    }
                })
                .collect(),
            total,
            failed_writes: service.failed_writes(),
        })
        .into_response(),
        Err(e) => {
            tracing::error!(code = "redemption.storage_failed", error = %e, "records not read");
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "redemption.storage_failed",
            )
        }
    }
}
