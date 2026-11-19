//! Admin API: the latest stored chat, so the admin window shows chat from before a restart. The caller adds the
//! token check.

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::SecondsFormat;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use utoipa::ToSchema;

use super::{ChatMessagePayload, db};

/// How many messages the admin window loads; the same as its live log keeps.
const RECENT: u32 = 200;

pub fn routes(pool: SqlitePool) -> Router {
    Router::new()
        .route("/api/admin/chat/messages", get(messages))
        .with_state(pool)
}

/// A stored chat message.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StoredChatMessage {
    /// Twitch's message ID, the same as the live `chat.message` event's `id`.
    pub id: String,
    /// RFC 3339.
    pub sent_at: String,
    /// A moderator took it down; its text and fragments are empty.
    pub deleted: bool,
    pub payload: ChatMessagePayload,
}

impl From<db::StoredMessage> for StoredChatMessage {
    fn from(m: db::StoredMessage) -> Self {
        Self {
            id: m.id,
            sent_at: m.sent_at.to_rfc3339_opts(SecondsFormat::Millis, true),
            deleted: m.deleted,
            payload: m.payload,
        }
    }
}

#[derive(Serialize, Deserialize, ToSchema)]
pub(crate) struct ErrorBody {
    code: String,
}

/// The latest 200 stored chat messages from Twitch, oldest first.
#[utoipa::path(
    get,
    path = "/api/admin/chat/messages",
    tag = "admin",
    responses((status = 200, body = Vec<StoredChatMessage>), (status = 500, body = ErrorBody))
)]
pub(crate) async fn messages(State(pool): State<SqlitePool>) -> Response {
    match db::recent_messages(&pool, RECENT).await {
        Ok(messages) => Json(
            messages
                .into_iter()
                .map(StoredChatMessage::from)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "stored chat not read");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorBody {
                    code: "chat.storage_failed".into(),
                }),
            )
                .into_response()
        }
    }
}
