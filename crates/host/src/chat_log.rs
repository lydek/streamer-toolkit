//! The chat log page: the periods to look back at (each stream, and the chat between streams), and one period's
//! messages with the follows, subs, cheers, raids and redemptions of that time. Each module answers from its own
//! table; nothing joins across them. Admin routes; the caller adds the token check.

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

use crate::admin::{ErrorBody, error};

/// How many streams the period list goes back.
const STREAMS: u32 = 50;
/// The most messages one period returns (the latest ones).
const MESSAGES: u32 = 5000;
/// The most follows, subs, raids and redemptions one period returns (the latest ones).
const EVENTS: u32 = 2000;

pub(crate) fn routes(pool: SqlitePool) -> Router {
    Router::new()
        .route("/api/admin/chat/periods", get(periods))
        .route("/api/admin/chat/log", get(log))
        .with_state(pool)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PeriodKind {
    /// A Twitch stream, from its start to its end.
    Stream,
    /// Chat between two streams (or before the first, or since the last ended).
    Offline,
}

/// A stretch of time the chat log can show.
#[derive(Debug, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatPeriod {
    kind: PeriodKind,
    /// RFC 3339 with microseconds, as stored, so a later query keeps the same boundary; `null` before the first
    /// stream.
    #[schema(required)]
    from: Option<String>,
    /// RFC 3339; `null` while it is still going.
    #[schema(required)]
    to: Option<String>,
    /// When its first stored message was sent; `null` when it has none.
    #[schema(required)]
    first_at: Option<String>,
    /// Stored chat messages in it.
    messages: u32,
}

/// Microseconds, as stored: a period's ends go back to the server as query bounds and must not move.
fn time_text(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// Keeps the latest `limit` of `rows` (oldest first, read with one more than `limit`); `true` when any was left out.
fn keep_latest<T>(rows: &mut Vec<T>, limit: u32) -> bool {
    let over = rows.len().saturating_sub(limit as usize);
    rows.drain(..over);
    over > 0
}

/// The latest streams, newest first, with the chat between them; stretches between streams with no chat are left
/// out.
#[utoipa::path(
    get,
    path = "/api/admin/chat/periods",
    tag = "admin",
    responses((status = 200, body = Vec<ChatPeriod>), (status = 500, body = ErrorBody))
)]
pub(crate) async fn periods(State(pool): State<SqlitePool>) -> Response {
    match list_periods(&pool).await {
        Ok(periods) => Json(periods).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "chat periods not read");
            error(StatusCode::INTERNAL_SERVER_ERROR, "chat.storage_failed")
        }
    }
}

async fn list_periods(pool: &SqlitePool) -> Result<Vec<ChatPeriod>, sqlx::Error> {
    // One more than listed, to know whether an older stream was left out.
    let mut streams = stream::sessions(pool, STREAMS + 1).await?;
    let older_left_out = streams.len() > STREAMS as usize;
    streams.truncate(STREAMS as usize);
    let mut periods = Vec::new();
    let offline = |from: Option<DateTime<Utc>>, to: Option<DateTime<Utc>>| async move {
        let (messages, first) = chat::count_between(pool, from, to).await?;
        Ok::<_, sqlx::Error>((messages > 0).then(|| ChatPeriod {
            kind: PeriodKind::Offline,
            from: from.map(time_text),
            to: to.map(time_text),
            first_at: first.map(time_text),
            messages,
        }))
    };
    // Newest first: the chat since the last stream ended, then each stream and the chat before it.
    let mut later_start: Option<DateTime<Utc>> = None;
    for (i, s) in streams.iter().enumerate() {
        if let Some(end) = s.ended_at
            && let Some(p) = offline(Some(end), later_start).await?
        {
            periods.push(p);
        }
        let (messages, first) = chat::count_between(pool, Some(s.started_at), s.ended_at).await?;
        periods.push(ChatPeriod {
            kind: PeriodKind::Stream,
            from: Some(time_text(s.started_at)),
            to: s.ended_at.map(time_text),
            first_at: first.map(time_text),
            messages,
        });
        later_start = Some(s.started_at);
        // Before the oldest listed stream, only when no older one was left out.
        if i + 1 == streams.len()
            && !older_left_out
            && let Some(p) = offline(None, later_start).await?
        {
            periods.push(p);
        }
    }
    if streams.is_empty()
        && let Some(p) = offline(None, None).await?
    {
        periods.push(p);
    }
    Ok(periods)
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct LogQuery {
    /// RFC 3339; open when left out.
    from: Option<String>,
    /// RFC 3339, not included; open when left out.
    to: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LogEventKind {
    Follow,
    Subscribe,
    Resubscribe,
    Gift,
    Cheer,
    Raid,
    Redemption,
}

/// A follow, sub, resub, gift, cheer, raid or redemption, placed among the messages by its time. Fields the kind
/// does not have are left out; names and texts are raw viewer text.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LogEvent {
    /// RFC 3339.
    at: String,
    kind: LogEventKind,
    /// `null` for an anonymous gift or cheer.
    #[schema(required)]
    user_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    months: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bits: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    viewers: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    /// A redemption's reward.
    #[serde(skip_serializing_if = "Option::is_none")]
    reward: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatLogBody {
    /// Oldest first.
    messages: Vec<chat::StoredChatMessage>,
    /// Oldest first.
    events: Vec<LogEvent>,
    /// More messages or events than one page holds happened then; these are the latest.
    truncated: bool,
}

/// The stored messages, follows, subs, cheers, raids and redemptions from `from` until `to`.
#[utoipa::path(
    get,
    path = "/api/admin/chat/log",
    tag = "admin",
    params(LogQuery),
    responses(
        (status = 200, body = ChatLogBody),
        (status = 400, body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
pub(crate) async fn log(State(pool): State<SqlitePool>, Query(q): Query<LogQuery>) -> Response {
    let parse = |t: Option<String>| {
        t.map(|t| DateTime::parse_from_rfc3339(&t).map(|t| t.to_utc()))
            .transpose()
    };
    let (Ok(from), Ok(to)) = (parse(q.from), parse(q.to)) else {
        return error(StatusCode::BAD_REQUEST, "chat_log.invalid_time");
    };
    match read_log(&pool, from, to).await {
        Ok(body) => Json(body).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "chat log not read");
            error(StatusCode::INTERNAL_SERVER_ERROR, "chat.storage_failed")
        }
    }
}

async fn read_log(
    pool: &SqlitePool,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
) -> Result<ChatLogBody, sqlx::Error> {
    // Each read asks for one more than it keeps, to know whether anything was left out.
    let mut messages = chat::messages_between(pool, from, to, MESSAGES + 1).await?;
    let mut activity = activity::activity_between(pool, from, to, EVENTS + 1).await?;
    let mut redeemed = redemption::redeemed_between(pool, from, to, EVENTS + 1).await?;
    let truncated = keep_latest(&mut messages, MESSAGES)
        | keep_latest(&mut activity, EVENTS)
        | keep_latest(&mut redeemed, EVENTS);
    let kind = |k: &str| match k {
        "follow" => Some(LogEventKind::Follow),
        "subscribe" => Some(LogEventKind::Subscribe),
        "resubscribe" => Some(LogEventKind::Resubscribe),
        "gift" => Some(LogEventKind::Gift),
        "cheer" => Some(LogEventKind::Cheer),
        "raid" => Some(LogEventKind::Raid),
        _ => None,
    };
    let mut events: Vec<(DateTime<Utc>, LogEvent)> = activity
        .into_iter()
        // A gift's subs are covered by the gift itself.
        .filter(|a| !a.gifted)
        .filter_map(|a| {
            Some((
                parse(&a.occurred_at)?,
                LogEvent {
                    at: String::new(),
                    kind: kind(&a.kind)?,
                    user_name: a.user_name,
                    months: a.months,
                    count: a.count,
                    bits: a.bits,
                    viewers: a.viewers,
                    message: a.message.filter(|m| !m.is_empty()),
                    reward: None,
                },
            ))
        })
        .chain(redeemed.into_iter().filter_map(|r| {
            Some((
                parse(&r.redeemed_at)?,
                LogEvent {
                    at: String::new(),
                    kind: LogEventKind::Redemption,
                    user_name: Some(r.user_name),
                    months: None,
                    count: None,
                    bits: None,
                    viewers: None,
                    message: Some(r.user_input).filter(|m| !m.is_empty()),
                    reward: Some(r.reward_title),
                },
            ))
        }))
        .collect();
    events.sort_by_key(|(at, _)| *at);
    Ok(ChatLogBody {
        messages: messages.into_iter().map(Into::into).collect(),
        events: events
            .into_iter()
            .map(|(at, e)| LogEvent {
                at: time_text(at),
                ..e
            })
            .collect(),
        truncated,
    })
}

fn parse(t: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(t).ok().map(|t| t.to_utc())
}
