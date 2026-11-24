//! Chat messages in SQLite (`chat_messages`).

use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection, SqlitePool, migrate::Migrator};
use toolkit_core::{
    events::ChatRemoval,
    records::{RecordFuture, RecordTable},
};

use super::{ChatMessagePayload, chat_payload};
use crate::application::{ChatRepository, Removal, RepositoryError, ToStore};

pub static MIGRATOR: Migrator = sqlx::migrate!();

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
    async fn store(&self, messages: &[ToStore]) -> Result<(), RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        for (m, session, first) in messages {
            // Twitch's time; a message without a readable one counts as arriving now.
            let sent_at = DateTime::parse_from_rfc3339(&m.sent_at)
                .map_or_else(|_| Utc::now(), |t| t.to_utc());
            let payload = serde_json::to_string(&ChatMessagePayload {
                first_chat: *first,
                ..chat_payload(m.clone())
            })
            .map_err(storage)?;
            sqlx::query(
                "INSERT INTO chat_messages \
                 (id, twitch_message_id, broadcaster_id, session_id, user_id, payload, first_chat, sent_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT (twitch_message_id) DO NOTHING",
            )
            .bind(uuid::Uuid::now_v7().to_string())
            .bind(&m.id)
            .bind(&m.broadcaster_id)
            .bind(session)
            .bind(&m.user_id)
            .bind(payload)
            .bind(first)
            .bind(time_text(sent_at))
            .execute(&mut *tx)
            .await
            .map_err(storage)?;
        }
        tx.commit().await.map_err(storage)
    }

    async fn remove(&self, removal: &Removal) -> Result<u64, RepositoryError> {
        // A Twitch message ID names one message wherever it was sent; a viewer or the whole chat only in its channel.
        let since = removal.since.map(time_text).unwrap_or_default();
        let channel = removal.channel.clone();
        let query = match &removal.removal {
            ChatRemoval::Message { id } => {
                sqlx::query(clear_where!("twitch_message_id = ?")).bind(id.clone())
            }
            ChatRemoval::User { user_id } => sqlx::query(clear_where!(
                "broadcaster_id = ? AND user_id = ? AND sent_at >= ?"
            ))
            .bind(channel)
            .bind(user_id.clone())
            .bind(since),
            ChatRemoval::All => sqlx::query(clear_where!("broadcaster_id = ? AND sent_at >= ?"))
                .bind(channel)
                .bind(since),
        };
        Ok(query
            .execute(&self.pool)
            .await
            .map_err(storage)?
            .rows_affected())
    }

    async fn mark_first(&self, id: &str) -> Result<(), RepositoryError> {
        self.mark(id).await.map_err(storage)
    }
}

impl SqliteChat {
    async fn mark(&self, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE chat_messages SET first_chat = 1, payload = json_set(payload, '$.firstChat', json('true')) \
             WHERE twitch_message_id = ?",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
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
/// The top `limit` chatters from `from` until `until` (open ends when `None`), most messages first: name and count.
/// Logins in `exclude` (lowercase) are left out; simulated messages are never stored. A viewer goes by the login and
/// name of their latest message in that time, so a rename shows.
pub async fn top_chatters(
    pool: &SqlitePool,
    from: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    exclude: &[&str],
    limit: u32,
) -> Result<Vec<(String, u64)>, sqlx::Error> {
    let rows: Vec<(String, String, i64)> = sqlx::query_as(
        "WITH ranked AS (SELECT user_id, payload, sent_at, \
         ROW_NUMBER() OVER (PARTITION BY user_id ORDER BY sent_at DESC, id DESC) AS latest \
         FROM chat_messages WHERE (?1 IS NULL OR sent_at >= ?1) AND (?2 IS NULL OR sent_at < ?2)) \
         SELECT LOWER(COALESCE(json_extract(MAX(CASE WHEN latest = 1 THEN payload END), '$.userLogin'), '')), \
         COALESCE(json_extract(MAX(CASE WHEN latest = 1 THEN payload END), '$.userName'), ''), COUNT(*) AS n \
         FROM ranked GROUP BY user_id ORDER BY n DESC, MIN(sent_at)",
    )
    .bind(from.map(time_text))
    .bind(until.map(time_text))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|(login, _, _)| !exclude.contains(&login.as_str()))
        .take(limit as usize)
        .map(|(_, name, n)| (name, u64::try_from(n).unwrap_or(0)))
        .collect())
}

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

/// How many messages and first chats are stored for the session, from the same read.
pub async fn session_chat_counts(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<(u32, u32), sqlx::Error> {
    let (messages, first): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(*) FILTER (WHERE first_chat = 1) FROM chat_messages WHERE session_id = ?",
    )
        .bind(session_id)
        .fetch_one(pool)
        .await?;
    Ok((
        messages.try_into().unwrap_or(u32::MAX),
        first.try_into().unwrap_or(u32::MAX),
    ))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use chrono::TimeDelta;
    use sqlx::sqlite::SqlitePoolOptions;
    use toolkit_core::events::{ChatMessage, Fragment};

    /// A removal in channel `42`, a viewer or the whole chat reaching back a day.
    fn removal(removal: ChatRemoval) -> Removal {
        Removal {
            channel: "42".into(),
            since: (!matches!(removal, ChatRemoval::Message { .. }))
                .then(|| Utc::now() - TimeDelta::hours(24)),
            removal,
        }
    }

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
            broadcaster_id: "42".into(),
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
            (message("m1", "7", "hi", &at(3)), Some("s1".into()), false),
            (
                message("m2", "8", "<b>yo</b>", &at(2)),
                Some("s1".into()),
                false,
            ),
        ])
        .await
        .unwrap();
        repo.store(&[
            (
                message("m1", "7", "hi again", &at(1)),
                Some("s1".into()),
                false,
            ),
            (message("m3", "7", "later", &at(1)), None, false),
        ])
        .await
        .unwrap();
        assert_eq!(session_chat_counts(&pool, "s1").await.unwrap().0, 2);
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
            (message("m1", "7", "one", &at(5)), None, false),
            (message("m2", "7", "two", &at(4)), None, false),
            (message("m3", "8", "three", &at(3)), None, false),
            (message("m4", "7", "old", &old), None, false),
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
            repo.remove(&removal(ChatRemoval::Message { id: "m3".into() }))
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            repo.remove(&removal(ChatRemoval::User {
                user_id: "7".into()
            }))
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
            repo.remove(&removal(ChatRemoval::All)).await.unwrap(),
            0,
            "already cleared"
        );
    }

    #[tokio::test]
    async fn a_message_after_a_clear_stays_until_the_next_clear() {
        let pool = pool().await;
        let repo = SqliteChat::new(pool.clone());
        let at = |mins: i64| time_text(Utc::now() - TimeDelta::minutes(mins));
        let deleted = |pool: SqlitePool| async move {
            recent_messages(&pool, 10)
                .await
                .unwrap()
                .into_iter()
                .map(|m| (m.id, m.deleted))
                .collect::<Vec<_>>()
        };
        repo.store(&[(message("m1", "7", "before", &at(2)), None, false)])
            .await
            .unwrap();
        repo.remove(&removal(ChatRemoval::All)).await.unwrap();
        repo.store(&[(message("m2", "7", "after", &at(1)), None, false)])
            .await
            .unwrap();
        assert_eq!(
            deleted(pool.clone()).await,
            [("m1".to_owned(), true), ("m2".to_owned(), false)],
            "a clear never reaches a message stored after it"
        );
        repo.remove(&removal(ChatRemoval::All)).await.unwrap();
        assert_eq!(
            deleted(pool.clone()).await,
            [("m1".to_owned(), true), ("m2".to_owned(), true)]
        );
    }

    #[tokio::test]
    async fn clearing_a_viewer_or_the_chat_stays_in_its_channel() {
        let pool = pool().await;
        let repo = SqliteChat::new(pool.clone());
        let at = time_text(Utc::now() - TimeDelta::minutes(5));
        let other = |id: &str| ChatMessage {
            broadcaster_id: "99".into(),
            ..message(id, "7", "elsewhere", &at)
        };
        repo.store(&[
            (message("a1", "7", "here", &at), None, false),
            (other("b1"), None, false),
            (message("a2", "8", "here too", &at), None, false),
            (other("b2"), None, false),
        ])
        .await
        .unwrap();
        // Stored before the channel was kept: no clear can tell whose it is, so none reaches it.
        sqlx::query(
            "UPDATE chat_messages SET broadcaster_id = NULL WHERE twitch_message_id = 'b2'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let user = ChatRemoval::User {
            user_id: "7".into(),
        };
        assert_eq!(repo.remove(&removal(user)).await.unwrap(), 1);
        assert_eq!(repo.remove(&removal(ChatRemoval::All)).await.unwrap(), 1);
        let mut deleted: Vec<(String, bool)> = recent_messages(&pool, 10)
            .await
            .unwrap()
            .into_iter()
            .map(|m| (m.id, m.deleted))
            .collect();
        deleted.sort();
        assert_eq!(
            deleted,
            [
                ("a1".to_owned(), true),
                ("a2".to_owned(), true),
                ("b1".to_owned(), false),
                ("b2".to_owned(), false)
            ]
        );
    }

    #[tokio::test]
    async fn messages_and_counts_between_two_times() {
        let pool = pool().await;
        let repo = SqliteChat::new(pool.clone());
        repo.store(&[
            (message("m1", "7", "a", "2026-11-19T09:00:00Z"), None, false),
            (message("m2", "7", "b", "2026-11-19T10:00:00Z"), None, false),
            (message("m3", "7", "c", "2026-11-19T11:00:00Z"), None, false),
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
            (
                message("m1", "7", "old", "2026-11-01T10:00:00Z"),
                None,
                false,
            ),
            (
                message("m2", "7", "new", "2026-11-19T10:00:00Z"),
                None,
                false,
            ),
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

    #[tokio::test]
    async fn chatters_go_by_their_latest_login_and_name_in_the_time_asked() {
        let pool = pool().await;
        // u1: an older name that sorts last, then two in the same instant (the later stored wins), then one after the
        // time asked; u2 says more.
        let rows = [
            ("01", "u1", "zoe", "Zoe", "2026-11-19T10:00:00.000000Z"),
            ("02", "u1", "amy", "Amy", "2026-11-19T10:05:00.000000Z"),
            ("03", "u1", "ann", "Ann", "2026-11-19T10:05:00.000000Z"),
            ("04", "u1", "bea", "Bea", "2026-11-19T11:00:00.000000Z"),
            ("05", "u2", "kai", "Kai", "2026-11-19T10:01:00.000000Z"),
            ("06", "u2", "kai", "Kai", "2026-11-19T10:02:00.000000Z"),
            ("07", "u2", "kai", "Kai", "2026-11-19T10:03:00.000000Z"),
            ("08", "u2", "kai", "Kai", "2026-11-19T10:04:00.000000Z"),
        ];
        for (id, user, login, name, at) in rows {
            sqlx::query(
                "INSERT INTO chat_messages (id, twitch_message_id, user_id, payload, sent_at) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(id)
            .bind(user)
            .bind(json_payload(login, name))
            .bind(at)
            .execute(&pool)
            .await
            .unwrap();
        }
        let at = |t: &str| DateTime::parse_from_rfc3339(t).unwrap().to_utc();
        let (from, until) = (
            Some(at("2026-11-19T09:00:00Z")),
            Some(at("2026-11-19T10:30:00Z")),
        );
        assert_eq!(
            top_chatters(&pool, from, until, &[], 10).await.unwrap(),
            [("Kai".to_owned(), 4), ("Ann".to_owned(), 3)]
        );
        assert_eq!(
            top_chatters(&pool, from, until, &["ann"], 10)
                .await
                .unwrap(),
            [("Kai".to_owned(), 4)]
        );
    }

    fn json_payload(login: &str, name: &str) -> String {
        serde_json::json!({ "userLogin": login, "userName": name, "text": "hi" }).to_string()
    }
}
