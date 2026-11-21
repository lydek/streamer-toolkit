//! The leaderboard (排行榜): bits, gifted subs, chat messages and check-ins, for this stream, this month and all
//! records kept. Each module ranks its own records; the Host only composes them, so nothing joins across modules.
//! 本月 starts with the check-in business day (time zone and reset hour); 全部 is whatever the record retention kept,
//! except check-ins, whose 全部 is the lifetime total the check-in page shows. Known chat bots are left out, and
//! simulated events are never stored. The boards are announced as `leaderboard.update` for the overlay; admin
//! routes need the token (the caller adds it).

use std::{sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use checkin::CheckinService;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use toolkit_core::{
    bus::{Event, EventBus},
    events::Source,
};
use utoipa::ToSchema;

use crate::admin::{ErrorBody, error};

/// Common chat bots, left out of the leaderboard and hidden by the chat log's 隱藏機器人.
// shortcut: a fixed list; a broadcaster's own list waits for viewer roles (phase 7).
pub(crate) const BOTS: [&str; 6] = [
    "nightbot",
    "streamelements",
    "streamlabs",
    "moobot",
    "fossabot",
    "sery_bot",
];
/// How many viewers each board lists.
const TOP: u32 = 10;
/// How often the boards are worked out again for the overlay.
const EVERY: Duration = Duration::from_secs(30);
const UPDATE: &str = "leaderboard.update";
const UPDATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BoardKind {
    Bits,
    Gifts,
    Chat,
    Checkins,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Period {
    /// The latest stream (since launch before the first one).
    Stream,
    Month,
    All,
}

/// One viewer on a board. `name` is viewer text: render it as text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub(crate) struct BoardEntry {
    pub(crate) name: String,
    pub(crate) value: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub(crate) struct Board {
    pub(crate) board: BoardKind,
    pub(crate) period: Period,
    /// Most first, at most 10.
    pub(crate) entries: Vec<BoardEntry>,
}

/// Every board for every period.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub(crate) struct LeaderboardPayload {
    pub(crate) boards: Vec<Board>,
}

/// A `leaderboard.update` event exactly as `/ws/overlay` sends it when the boards change (checked every 30 s); the
/// snapshot keeps only the latest one.
// The bus serializes its own envelope; a test keeps this type in step with it.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LeaderboardEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: LeaderboardType,
    version: u32,
    seq: u64,
    occurred_at: String,
    source: Source,
    payload: LeaderboardPayload,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
enum LeaderboardType {
    #[serde(rename = "leaderboard.update")]
    LeaderboardUpdate,
}

#[derive(Clone)]
pub(crate) struct LeaderboardState {
    pub(crate) pool: SqlitePool,
    pub(crate) checkin: Arc<CheckinService>,
    pub(crate) launched_at: DateTime<Utc>,
}

/// Works out every board now.
pub(crate) async fn boards(
    s: &LeaderboardState,
    now: DateTime<Utc>,
) -> Result<LeaderboardPayload, sqlx::Error> {
    // The latest stream, live or ended; events after it ended belong to no stream's board.
    let (stream_from, stream_until) = stream::latest_session(&s.pool)
        .await?
        .map_or((s.launched_at, None), |session| {
            (session.started_at, session.ended_at)
        });
    let mut boards = Vec::new();
    for (period, from, until) in [
        (Period::Stream, Some(stream_from), stream_until),
        (Period::Month, Some(s.checkin.month_start(now)), None),
        (Period::All, None, None),
    ] {
        for board in [
            BoardKind::Bits,
            BoardKind::Gifts,
            BoardKind::Chat,
            BoardKind::Checkins,
        ] {
            let ranked = match board {
                BoardKind::Bits => {
                    activity::ranking(&s.pool, activity::Ranking::Bits, from, until, &BOTS, TOP)
                        .await?
                }
                BoardKind::Gifts => {
                    activity::ranking(&s.pool, activity::Ranking::Gifts, from, until, &BOTS, TOP)
                        .await?
                }
                BoardKind::Chat => chat::top_chatters(&s.pool, from, until, &BOTS, TOP).await?,
                BoardKind::Checkins => {
                    checkin::top_checkins(&s.pool, from, until, &BOTS, TOP).await?
                }
            };
            boards.push(Board {
                board,
                period,
                entries: ranked
                    .into_iter()
                    .map(|(name, value)| BoardEntry { name, value })
                    .collect(),
            });
        }
    }
    Ok(LeaderboardPayload { boards })
}

/// Announces the boards now and whenever they change, checked every 30 s. Needs a Tokio runtime.
pub(crate) fn start(state: LeaderboardState, bus: Arc<EventBus>) {
    bus.retain(UPDATE, 1);
    tokio::spawn(async move {
        let mut last = None;
        let mut tick = tokio::time::interval(EVERY);
        loop {
            tick.tick().await;
            match boards(&state, Utc::now()).await {
                Ok(next) if last.as_ref() != Some(&next) => {
                    bus.publish(Event {
                        id: uuid::Uuid::now_v7().to_string(),
                        event_type: UPDATE,
                        version: UPDATE_VERSION,
                        occurred_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                        source: Source::System,
                        payload: serde_json::to_value(&next).expect("leaderboard serializes"),
                    });
                    last = Some(next);
                }
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "leaderboard not worked out"),
            }
        }
    });
}

pub(crate) fn routes(state: LeaderboardState) -> Router {
    Router::new()
        .route("/api/admin/leaderboard", get(leaderboard))
        .route("/api/admin/chat/bots", get(bots))
        .with_state(state)
}

#[utoipa::path(
    get,
    path = "/api/admin/leaderboard",
    tag = "admin",
    responses(
        (status = 200, body = LeaderboardPayload),
        (status = 500, description = "`leaderboard.storage_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn leaderboard(State(s): State<LeaderboardState>) -> Response {
    match boards(&s, Utc::now()).await {
        Ok(b) => Json(b).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "leaderboard not read");
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "leaderboard.storage_failed",
            )
        }
    }
}

/// The chat bots the leaderboard leaves out and the chat log can hide (lowercase logins).
#[utoipa::path(
    get,
    path = "/api/admin/chat/bots",
    tag = "admin",
    responses((status = 200, body = Vec<String>))
)]
pub(crate) async fn bots() -> Json<Vec<String>> {
    Json(BOTS.iter().map(|b| (*b).to_owned()).collect())
}
