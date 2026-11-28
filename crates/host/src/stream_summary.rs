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
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use utoipa::ToSchema;

use crate::admin::{ErrorBody, error};
use toolkit_core::time::{self, Timestamp};

#[derive(Clone)]
pub(crate) struct SummaryState {
    pool: SqlitePool,
    /// When this Host started: a stream that ended after it ended "just now" (home shows 下播後).
    launched_at: Timestamp,
}

pub(crate) fn routes(pool: SqlitePool, launched_at: Timestamp) -> Router {
    Router::new()
        .route("/api/admin/stream/summary", get(stream_summary))
        .route("/api/admin/stream/record", get(stream_record))
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

fn time_text(t: Timestamp) -> String {
    time::millis(t)
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

/// One row of the stream's record: what the stats count, one row each.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecordKind {
    Follow,
    Subscribe,
    Resubscribe,
    Cheer,
    Raid,
    Redemption,
}

/// One row; names, messages, reward titles and inputs are raw viewer text.
#[derive(Debug, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecordRow {
    /// RFC 3339, UTC.
    at: String,
    kind: RecordKind,
    /// `null` for an anonymous sub or cheer.
    #[schema(required)]
    viewer: Option<String>,
    /// 1, 2 or 3, for a sub or resub.
    #[schema(required)]
    tier: Option<u8>,
    /// A sub handed out by a gift.
    gifted: bool,
    #[schema(required)]
    months: Option<u32>,
    #[schema(required)]
    bits: Option<u32>,
    /// Raiders.
    #[schema(required)]
    viewers: Option<u32>,
    /// A resub's or cheer's message.
    #[schema(required)]
    message: Option<String>,
    /// A redemption's reward title.
    #[schema(required)]
    reward: Option<String>,
    /// What the viewer typed for the reward; `null` when nothing.
    #[schema(required)]
    input: Option<String>,
}

/// The latest stream session and what its stats count, row by row, to export.
#[derive(Debug, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StreamRecord {
    started_at: String,
    #[schema(required)]
    ended_at: Option<String>,
    /// Oldest first.
    rows: Vec<RecordRow>,
}

/// The latest stream's follows, subs and resubs (each gifted sub included), cheers, raids and redemptions, one row
/// each, as its stats count them; `null` before the first stream. Simulated events are never stored, so never
/// here.
#[utoipa::path(
    get,
    path = "/api/admin/stream/record",
    tag = "admin",
    responses(
        (status = 200, body = Option<StreamRecord>),
        (status = 500, body = ErrorBody),
    )
)]
pub(crate) async fn stream_record(State(s): State<SummaryState>) -> Response {
    match record(&s.pool).await {
        Ok(record) => Json(record).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "stream record not read");
            error(StatusCode::INTERNAL_SERVER_ERROR, "stream.storage_failed")
        }
    }
}

async fn record(pool: &SqlitePool) -> Result<Option<StreamRecord>, sqlx::Error> {
    let Some(session) = stream::latest_session(pool).await? else {
        return Ok(None);
    };
    let at = |t: &str| {
        t.parse::<Timestamp>()
            .map(time_text)
            .unwrap_or_else(|_| t.to_owned())
    };
    let mut rows: Vec<RecordRow> = activity::session_entries(pool, &session.id)
        .await?
        .into_iter()
        .filter_map(|e| {
            let kind = match e.kind.as_str() {
                "follow" => RecordKind::Follow,
                "subscribe" => RecordKind::Subscribe,
                "resubscribe" => RecordKind::Resubscribe,
                "cheer" => RecordKind::Cheer,
                "raid" => RecordKind::Raid,
                _ => return None,
            };
            Some(RecordRow {
                at: at(&e.occurred_at),
                kind,
                viewer: e.user_name,
                tier: e.tier,
                gifted: e.gifted,
                months: e.months,
                bits: e.bits,
                viewers: e.viewers,
                message: e.message.filter(|m| !m.is_empty()),
                reward: None,
                input: None,
            })
        })
        .collect();
    let redemptions =
        redemption::redeemed_between(pool, Some(session.started_at), session.ended_at, u32::MAX)
            .await?;
    rows.extend(redemptions.into_iter().map(|r| RecordRow {
        at: at(&r.redeemed_at),
        kind: RecordKind::Redemption,
        viewer: Some(r.user_name),
        tier: None,
        gifted: false,
        months: None,
        bits: None,
        viewers: None,
        message: None,
        reward: Some(r.reward_title),
        input: Some(r.user_input).filter(|i| !i.is_empty()),
    }));
    // Fixed-width times: text order is time order.
    rows.sort_by(|a, b| a.at.cmp(&b.at));
    Ok(Some(StreamRecord {
        started_at: time_text(session.started_at),
        ended_at: session.ended_at.map(time_text),
        rows,
    }))
}

async fn summary(
    pool: &SqlitePool,
    launched_at: Timestamp,
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

#[cfg(test)]
mod tests {
    use super::*;

    // One tuple per stored event reads more plainly than a struct made only for this seed.
    #[allow(clippy::type_complexity)]
    async fn seed(pool: &SqlitePool) {
        sqlx::query(
            "INSERT INTO stream_sessions (id, twitch_stream_id, started_at, ended_at) VALUES \
             ('old', 't0', '2026-11-20T10:00:00.000000Z', '2026-11-20T12:00:00.000000Z'), \
             ('s1', 't1', '2026-11-21T10:00:00.000000Z', '2026-11-21T12:00:00.000000Z')",
        )
        .execute(pool)
        .await
        .unwrap();
        // (id, kind, name, tier, months, count, bits, viewers, gifted, message, session, minute)
        let rows: [(
            &str,
            &str,
            Option<&str>,
            Option<u8>,
            Option<u32>,
            Option<u32>,
            Option<u32>,
            Option<u32>,
            bool,
            Option<&str>,
            &str,
            u32,
        ); 9] = [
            (
                "e1",
                "follow",
                Some("=cmd"),
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                "s1",
                1,
            ),
            (
                "e2",
                "subscribe",
                Some("阿凱"),
                Some(1),
                None,
                None,
                None,
                None,
                false,
                None,
                "s1",
                2,
            ),
            (
                "e3",
                "gift",
                Some("橘子"),
                Some(1),
                None,
                Some(2),
                None,
                None,
                false,
                None,
                "s1",
                3,
            ),
            (
                "e4",
                "subscribe",
                Some("小魚"),
                Some(1),
                None,
                None,
                None,
                None,
                true,
                None,
                "s1",
                3,
            ),
            (
                "e5",
                "subscribe",
                Some("莓莓"),
                Some(1),
                None,
                None,
                None,
                None,
                true,
                None,
                "s1",
                3,
            ),
            (
                "e6",
                "resubscribe",
                Some("阿凱"),
                Some(2),
                Some(6),
                None,
                None,
                None,
                false,
                Some("+1 嗨"),
                "s1",
                4,
            ),
            (
                "e7",
                "cheer",
                None,
                None,
                None,
                None,
                Some(150),
                None,
                false,
                Some(""),
                "s1",
                5,
            ),
            (
                "e8",
                "raid",
                Some("nightowl"),
                None,
                None,
                None,
                None,
                Some(12),
                false,
                None,
                "s1",
                6,
            ),
            (
                "e9",
                "follow",
                Some("昨天"),
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                "old",
                7,
            ),
        ];
        for (
            id,
            kind,
            name,
            tier,
            months,
            count,
            bits,
            viewers,
            gifted,
            message,
            session,
            minute,
        ) in rows
        {
            let day = if session == "old" { 20 } else { 21 };
            sqlx::query(
                "INSERT INTO activity_events (id, twitch_message_id, kind, user_name, tier, months, count, bits, \
                 viewers, gifted, message, session_id, occurred_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(id)
            .bind(kind)
            .bind(name)
            .bind(tier)
            .bind(months)
            .bind(count)
            .bind(bits)
            .bind(viewers)
            .bind(gifted)
            .bind(message)
            .bind(session)
            .bind(format!("2026-11-{day}T10:{minute:02}:00.000000Z"))
            .execute(pool)
            .await
            .unwrap();
        }
        for (id, at, input) in [
            ("r1", "2026-11-21T10:08:00.000000Z", "@everyone"),
            ("r2", "2026-11-21T13:00:00.000000Z", ""),
        ] {
            sqlx::query(
                "INSERT INTO redemption_records (id, twitch_redemption_id, user_id, user_login, user_name, reward_id, \
                 reward_title, reward_cost, user_input, status, redeemed_at, updated_at) \
                 VALUES (?, ?, '1', 'fish', '小魚', 'w', '喝水', 100, ?, 'unfulfilled', ?, ?)",
            )
            .bind(id)
            .bind(id)
            .bind(input)
            .bind(at)
            .bind(at)
            .execute(pool)
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn the_record_has_one_row_for_each_thing_the_stats_count() {
        let pool = crate::db::memory_migrated().await;
        assert_eq!(record(&pool).await.unwrap(), None, "no stream yet");
        seed(&pool).await;
        let record = record(&pool).await.unwrap().unwrap();
        let stats = summary(&pool, Timestamp::now()).await.unwrap().unwrap();
        let count = |kinds: &[RecordKind]| {
            record
                .rows
                .iter()
                .filter(|r| kinds.contains(&r.kind))
                .count() as u32
        };
        assert_eq!(count(&[RecordKind::Follow]), stats.follows);
        assert_eq!(
            count(&[RecordKind::Subscribe, RecordKind::Resubscribe]),
            stats.subs
        );
        assert_eq!(count(&[RecordKind::Raid]), stats.raids);
        assert_eq!(count(&[RecordKind::Redemption]), stats.redemptions);
        let bits: u32 = record.rows.iter().filter_map(|r| r.bits).sum();
        assert_eq!(bits, stats.bits);
        assert_eq!(
            (
                stats.follows,
                stats.subs,
                stats.bits,
                stats.raids,
                stats.redemptions
            ),
            (1, 4, 150, 1, 1)
        );

        let kinds: Vec<_> = record.rows.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            [
                RecordKind::Follow,
                RecordKind::Subscribe,
                RecordKind::Subscribe,
                RecordKind::Subscribe,
                RecordKind::Resubscribe,
                RecordKind::Cheer,
                RecordKind::Raid,
                RecordKind::Redemption,
            ],
            "oldest first; the gift itself and other streams left out"
        );
        assert_eq!(
            record.rows[0].viewer.as_deref(),
            Some("=cmd"),
            "raw text: the export makes it safe"
        );
        assert!(record.rows[2].gifted);
        assert_eq!(
            (
                record.rows[5].viewer.clone(),
                record.rows[5].message.clone()
            ),
            (None, None)
        );
        assert_eq!(record.rows[7].input.as_deref(), Some("@everyone"));
        assert_eq!(record.rows[0].at, "2026-11-21T10:01:00.000Z");
    }
}
