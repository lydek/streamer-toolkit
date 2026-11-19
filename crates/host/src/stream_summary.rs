//! This stream's numbers for the home page's stage card: the latest stream session (live, or the last one until the
//! next starts) with what the activity, redemption and chat modules recorded during it. Each module answers for its own
//! table; nothing joins across them. Admin routes; the caller adds the token check.

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use utoipa::ToSchema;

use crate::admin::{ErrorBody, error};

#[derive(Clone)]
pub(crate) struct SummaryState {
    pool: SqlitePool,
    /// When this Host started: a stream that ended after it ended "just now" (home shows 下播後).
    launched_at: DateTime<Utc>,
}

pub(crate) fn routes(pool: SqlitePool, launched_at: DateTime<Utc>) -> Router {
    Router::new()
        .route("/api/admin/stream/summary", get(stream_summary))
        .with_state(SummaryState { pool, launched_at })
}

#[derive(Debug, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StreamSummary {
    /// Twitch's start time (RFC 3339).
    started_at: String,
    /// `null` while live (or until the end is seen).
    ended_at: Option<String>,
    /// The stream ended while this Host was running.
    ended_since_launch: bool,
    /// The most viewers a check counted; `null` until one has.
    peak_viewers: Option<u32>,
    follows: u32,
    /// New subs and resubs, each gifted sub included.
    subs: u32,
    bits: u32,
    raids: u32,
    redemptions: u32,
    /// Chat messages from Twitch stored for the session.
    chat_messages: u32,
    /// Viewers' first messages in the channel (第一次聊天) during the session.
    first_chats: u32,
}

fn time_text(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// The latest stream session and what happened during it; `null` before the first stream.
#[utoipa::path(
    get,
    path = "/api/admin/stream/summary",
    tag = "admin",
    responses(
        (status = 200, body = Option<StreamSummary>),
        (status = 500, body = ErrorBody),
    )
)]
pub(crate) async fn stream_summary(State(s): State<SummaryState>) -> Response {
    match summary(&s.pool, s.launched_at).await {
        Ok(summary) => Json(summary).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "stream summary not read");
            error(StatusCode::INTERNAL_SERVER_ERROR, "stream.storage_failed")
        }
    }
}

async fn summary(
    pool: &SqlitePool,
    launched_at: DateTime<Utc>,
) -> Result<Option<StreamSummary>, sqlx::Error> {
    let Some(session) = stream::latest_session(pool).await? else {
        return Ok(None);
    };
    let counts = activity::session_counts(pool, &session.id).await?;
    let redemptions = redemption::count_between(pool, session.started_at, session.ended_at).await?;
    let (chat_messages, first_chats) = chat::session_chat_counts(pool, &session.id).await?;
    Ok(Some(StreamSummary {
        started_at: time_text(session.started_at),
        ended_at: session.ended_at.map(time_text),
        ended_since_launch: session.ended_at.is_some_and(|t| t >= launched_at),
        peak_viewers: session.peak_viewers,
        follows: counts.follows,
        subs: counts.subs,
        bits: counts.bits,
        raids: counts.raids,
        redemptions: u32::try_from(redemptions).unwrap_or(u32::MAX),
        chat_messages,
        first_chats,
    }))
}
