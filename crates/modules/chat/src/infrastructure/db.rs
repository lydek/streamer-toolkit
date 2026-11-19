//! Chat messages in SQLite (`chat_messages`).

use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection, SqlitePool, migrate::Migrator};
use toolkit_core::{
    events::{ChatMessage, ChatRemoval},
    records::{RecordFuture, RecordTable},
};

use super::{ChatMessagePayload, chat_payload};
use crate::application::{ChatRepository, RepositoryError};

pub static MIGRATOR: Migrator = sqlx::migrate!();

/// Clearing a viewer or the whole chat reaches back this far: Twitch clears what its chat window still shows, not
/// the broadcaster's history. shortcut: a fixed window, not Twitch's own; widen it if older messages show up.
const CLEAR_REACH: TimeDelta = TimeDelta::hours(24);

/// Empties the text and fragments of the not yet deleted messages `$filter` picks, and marks them deleted.
macro_rules! clear_where {
    ($filter:literal) => {
        concat!(
            "UPDATE chat_messages SET payload = json_set(payload, '$.text', '', '$.fragments', json('[]')), ",
            "deleted = 1 WHERE deleted = 0 AND ",
            $filter
        )
    };
}

pub(crate) struct SqliteChat {
    pool: SqlitePool,
}

impl SqliteChat {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn storage(e: impl std::fmt::Display) -> RepositoryError {
    RepositoryError(e.to_string())
}

/// Fixed width, so text order is time order.
fn time_text(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Micros, true)
}

impl ChatRepository for SqliteChat {
    async fn store(
        &self,
        messages: &[(ChatMessage, Option<String>)],
    ) -> Result<(), RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        for (m, session) in messages {
            // Twitch's time; a message without a readable one counts as arriving now.
            let sent_at = DateTime::parse_from_rfc3339(&m.sent_at)
                .map_or_else(|_| Utc::now(), |t| t.to_utc());
            let payload = serde_json::to_string(&chat_payload(m.clone())).map_err(storage)?;
            sqlx::query(
                "INSERT INTO chat_messages (id, twitch_message_id, session_id, user_id, payload, sent_at) \
                 VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT (twitch_message_id) DO NOTHING",
            )
            .bind(uuid::Uuid::now_v7().to_string())
            .bind(&m.id)
            .bind(session)
            .bind(&m.user_id)
            .bind(payload)
            .bind(time_text(sent_at))
            .execute(&mut *tx)
            .await
            .map_err(storage)?;
        }
        tx.commit().await.map_err(storage)
    }

    async fn remove(&self, removal: &ChatRemoval) -> Result<u64, RepositoryError> {
        let since = time_text(Utc::now() - CLEAR_REACH);
        let query = match removal {
            ChatRemoval::Message { id } => {
                sqlx::query(clear_where!("twitch_message_id = ?")).bind(id.clone())
            }
            ChatRemoval::User { user_id } => {
                sqlx::query(clear_where!("user_id = ? AND sent_at >= ?"))
                    .bind(user_id.clone())
                    .bind(since)
            }
            ChatRemoval::All => sqlx::query(clear_where!("sent_at >= ?")).bind(since),
        };
        Ok(query
            .execute(&self.pool)
            .await
            .map_err(storage)?
            .rows_affected())
    }
}

/// The chat table for the shared record retention.
pub struct ChatRecords;

impl RecordTable for ChatRecords {
    fn delete_before<'c>(
        &'c self,
        conn: &'c mut SqliteConnection,
        cutoff: &'c str,
    ) -> RecordFuture<'c> {
        Box::pin(async move {
            Ok(sqlx::query("DELETE FROM chat_messages WHERE sent_at < ?")
                .bind(cutoff)
                .execute(conn)
                .await?
                .rows_affected())
        })
    }

    fn count_before<'c>(
        &'c self,
        conn: &'c mut SqliteConnection,
        cutoff: &'c str,
    ) -> RecordFuture<'c> {
        Box::pin(async move {
            let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_messages WHERE sent_at < ?")
                .bind(cutoff)
                .fetch_one(conn)
                .await?;
            Ok(n.try_into().unwrap_or(0))
        })
    }
}

/// A stored message as the admin window reads it back.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredMessage {
    /// Twitch's message ID (the `chat.message` event's `id`).
    pub id: String,
    pub sent_at: DateTime<Utc>,
    pub deleted: bool,
    pub payload: ChatMessagePayload,
}

/// The latest `limit` stored messages, oldest first.
pub async fn recent_messages(
    pool: &SqlitePool,
    limit: u32,
) -> Result<Vec<StoredMessage>, sqlx::Error> {
    let rows: Vec<(String, String, bool, String)> = sqlx::query_as(
        "SELECT twitch_message_id, sent_at, deleted, payload FROM \
         (SELECT * FROM chat_messages ORDER BY sent_at DESC LIMIT ?) ORDER BY sent_at",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(stored).collect()
}

/// Stored messages from `from` until `until` (open ends when `None`), oldest first; at most the latest `limit`.
pub async fn messages_between(
    pool: &SqlitePool,
    from: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    limit: u32,
) -> Result<Vec<StoredMessage>, sqlx::Error> {
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT twitch_message_id, sent_at, deleted, payload FROM \
         (SELECT * FROM chat_messages",
    );
    push_time_range(&mut query, from, until);
    query
        .push(" ORDER BY sent_at DESC LIMIT ")
        .push_bind(limit)
        .push(") ORDER BY sent_at");
    let rows: Vec<(String, String, bool, String)> = query.build_query_as().fetch_all(pool).await?;
    rows.into_iter().map(stored).collect()
}

fn push_time_range(
    query: &mut QueryBuilder<Sqlite>,
    from: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
) {
    if let Some(from) = from {
        query.push(" WHERE sent_at >= ").push_bind(time_text(from));
    }
    if let Some(until) = until {
        query
            .push(if from.is_some() { " AND " } else { " WHERE " })
            .push("sent_at < ")
            .push_bind(time_text(until));
    }
}

/// How many messages were sent from `from` until `until` (open ends when `None`), and when the first was.
pub async fn count_between(
    pool: &SqlitePool,
    from: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
) -> Result<(u32, Option<DateTime<Utc>>), sqlx::Error> {
    let mut query = QueryBuilder::<Sqlite>::new("SELECT COUNT(*), MIN(sent_at) FROM chat_messages");
    push_time_range(&mut query, from, until);
    let (n, first): (i64, Option<String>) = query.build_query_as().fetch_one(pool).await?;
    let first = first
        .map(|s| DateTime::parse_from_rfc3339(&s).map(|t| t.to_utc()))
        .transpose()
        .map_err(|e| sqlx::Error::Decode(e.into()))?;
    Ok((n.try_into().unwrap_or(u32::MAX), first))
}

fn stored(
    (id, sent_at, deleted, payload): (String, String, bool, String),
) -> Result<StoredMessage, sqlx::Error> {
    Ok(StoredMessage {
        id,
        sent_at: DateTime::parse_from_rfc3339(&sent_at)
            .map_err(|e| sqlx::Error::Decode(e.into()))?
            .to_utc(),
        deleted,
        payload: serde_json::from_str(&payload).map_err(|e| sqlx::Error::Decode(e.into()))?,
    })
}

/// How many messages are stored for `session_id`.
pub async fn session_message_count(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<u32, sqlx::Error> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_messages WHERE session_id = ?")
        .bind(session_id)
        .fetch_one(pool)
        .await?;
    Ok(n.try_into().unwrap_or(u32::MAX))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use toolkit_core::events::Fragment;

    pub(crate) async fn pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        MIGRATOR.run(&pool).await.unwrap();
        pool
    }

    pub(crate) fn message(id: &str, user_id: &str, text: &str, sent_at: &str) -> ChatMessage {
        ChatMessage {
            id: id.into(),
            user_id: user_id.into(),
            user_login: format!("u{user_id}"),
            user_name: format!("U{user_id}"),
            color: None,
            text: text.into(),
            fragments: vec![Fragment::Text(text.into())],
            badges: Vec::new(),
            highlighted: false,
            bits: 0,
            reply: None,
            twitch_badges: Vec::new(),
            shared_from: None,
            sent_at: sent_at.into(),
        }
    }

    #[tokio::test]
    async fn stores_once_counts_per_session_and_reads_back_oldest_first() {
        let pool = pool().await;
        let repo = SqliteChat::new(pool.clone());
        let now = Utc::now();
        let at = |mins: i64| time_text(now - TimeDelta::minutes(mins));
        repo.store(&[
            (message("m1", "7", "hi", &at(3)), Some("s1".into())),
            (message("m2", "8", "<b>yo</b>", &at(2)), Some("s1".into())),
        ])
        .await
        .unwrap();
        repo.store(&[
            (message("m1", "7", "hi again", &at(1)), Some("s1".into())),
            (message("m3", "7", "later", &at(1)), None),
        ])
        .await
        .unwrap();
        assert_eq!(session_message_count(&pool, "s1").await.unwrap(), 2);
        let recent = recent_messages(&pool, 2).await.unwrap();
        let ids: Vec<&str> = recent.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["m2", "m3"], "the latest two, oldest first");
        assert_eq!(recent[0].payload.text, "<b>yo</b>", "viewer text stays raw");
        assert_eq!(
            recent_messages(&pool, 10).await.unwrap()[0].payload.text,
            "hi",
            "a repeat is not stored twice"
        );
    }

    #[tokio::test]
    async fn removals_clear_the_text_and_keep_the_message() {
        let pool = pool().await;
        let repo = SqliteChat::new(pool.clone());
        let now = Utc::now();
        let at = |mins: i64| time_text(now - TimeDelta::minutes(mins));
        let old = time_text(now - TimeDelta::days(3));
        repo.store(&[
            (message("m1", "7", "one", &at(5)), None),
            (message("m2", "7", "two", &at(4)), None),
            (message("m3", "8", "three", &at(3)), None),
            (message("m4", "7", "old", &old), None),
        ])
        .await
        .unwrap();
        let texts = |pool: SqlitePool| async move {
            recent_messages(&pool, 10)
                .await
                .unwrap()
                .into_iter()
                .map(|m| (m.id, m.payload.text, m.payload.fragments.len(), m.deleted))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            repo.remove(&ChatRemoval::Message { id: "m3".into() })
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            repo.remove(&ChatRemoval::User {
                user_id: "7".into()
            })
            .await
            .unwrap(),
            2
        );
        assert_eq!(
            texts(pool.clone()).await,
            [
                ("m4".to_owned(), "old".to_owned(), 1, false),
                ("m1".to_owned(), String::new(), 0, true),
                ("m2".to_owned(), String::new(), 0, true),
                ("m3".to_owned(), String::new(), 0, true),
            ],
            "older than a day stays"
        );
        assert_eq!(
            repo.remove(&ChatRemoval::All).await.unwrap(),
            0,
            "already cleared"
        );
    }

    #[tokio::test]
    async fn messages_and_counts_between_two_times() {
        let pool = pool().await;
        let repo = SqliteChat::new(pool.clone());
        repo.store(&[
            (message("m1", "7", "a", "2026-11-19T09:00:00Z"), None),
            (message("m2", "7", "b", "2026-11-19T10:00:00Z"), None),
            (message("m3", "7", "c", "2026-11-19T11:00:00Z"), None),
        ])
        .await
        .unwrap();
        let at = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().to_utc();
        let ids = |v: Vec<StoredMessage>| v.into_iter().map(|m| m.id).collect::<Vec<_>>();
        assert_eq!(
            ids(
                messages_between(&pool, Some(at("2026-11-19T10:00:00Z")), None, 10)
                    .await
                    .unwrap()
            ),
            ["m2", "m3"]
        );
        assert_eq!(
            ids(
                messages_between(&pool, None, Some(at("2026-11-19T10:00:00Z")), 10)
                    .await
                    .unwrap()
            ),
            ["m1"]
        );
        assert_eq!(
            ids(messages_between(&pool, None, None, 2).await.unwrap()),
            ["m2", "m3"],
            "the latest, oldest first"
        );
        assert_eq!(
            count_between(&pool, Some(at("2026-11-19T09:30:00Z")), None)
                .await
                .unwrap(),
            (2, Some(at("2026-11-19T10:00:00Z")))
        );
        assert_eq!(
            count_between(&pool, Some(at("2026-11-20T00:00:00Z")), None)
                .await
                .unwrap(),
            (0, None)
        );
        let from = Some(at("2026-11-19T09:00:00Z"));
        let until = Some(at("2026-11-19T11:00:00Z"));
        assert_eq!(
            ids(messages_between(&pool, from, until, 10).await.unwrap()),
            ["m1", "m2"]
        );
        assert_eq!(count_between(&pool, from, until).await.unwrap(), (2, from));
    }

    #[tokio::test]
    async fn the_retention_deletes_and_counts_old_messages() {
        let pool = pool().await;
        let repo = SqliteChat::new(pool.clone());
        repo.store(&[
            (message("m1", "7", "old", "2026-11-01T10:00:00Z"), None),
            (message("m2", "7", "new", "2026-11-19T10:00:00Z"), None),
        ])
        .await
        .unwrap();
        let mut conn = pool.acquire().await.unwrap();
        let cutoff = "2026-11-10T00:00:00.000000Z";
        assert_eq!(
            ChatRecords.count_before(&mut conn, cutoff).await.unwrap(),
            1
        );
        assert_eq!(
            ChatRecords.delete_before(&mut conn, cutoff).await.unwrap(),
            1
        );
        assert_eq!(
            ChatRecords.count_before(&mut conn, cutoff).await.unwrap(),
            0
        );
    }
}
