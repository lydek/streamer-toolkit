//! Activity records in SQLite (`activity_events`).

use std::sync::Arc;

use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::{SqliteConnection, SqlitePool, migrate::Migrator};
use toolkit_core::records::RecordTable;

use crate::{
    application::{ActivityRepository, RepositoryError},
    domain::Record,
};

pub static MIGRATOR: Migrator = sqlx::migrate!();

/// Other modules' record tables that share the retention (e.g. chat).
pub type OtherRecords = Vec<Arc<dyn RecordTable>>;

pub(crate) struct SqliteActivity {
    pool: SqlitePool,
    others: OtherRecords,
}

impl SqliteActivity {
    pub(crate) fn new(pool: SqlitePool, others: OtherRecords) -> Self {
        Self { pool, others }
    }

    /// Deletes from this module's table and every other one on `conn`.
    async fn delete_all(
        &self,
        conn: &mut SqliteConnection,
        cutoff: &str,
    ) -> Result<u64, sqlx::Error> {
        let mut deleted = sqlx::query("DELETE FROM activity_events WHERE occurred_at < ?")
            .bind(cutoff)
            .execute(&mut *conn)
            .await?
            .rows_affected();
        for table in &self.others {
            deleted += table.delete_before(conn, cutoff).await?;
        }
        Ok(deleted)
    }
}

fn storage(e: impl std::fmt::Display) -> RepositoryError {
    RepositoryError(e.to_string())
}

/// Fixed width, so text order is time order.
fn time_text(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// What happened in one stream session, as the host's stream summary shows it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionCounts {
    pub follows: u32,
    /// New subs and resubs, each sub a gift hands out included (the gift itself is not counted again).
    pub subs: u32,
    pub bits: u32,
    pub raids: u32,
}

/// One stored follow, sub, resub, gift, cheer or raid, as the chat log and the event feed merge it in.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ActivityEntry {
    /// The alert's ID (Twitch's message ID), as the alerts module knows it.
    pub alert_id: String,
    /// `follow`, `subscribe`, `resubscribe`, `gift`, `cheer` or `raid`.
    pub kind: String,
    /// `None` for an anonymous gift or cheer.
    pub user_name: Option<String>,
    pub tier: Option<u8>,
    pub months: Option<u32>,
    pub count: Option<u32>,
    pub bits: Option<u32>,
    pub viewers: Option<u32>,
    /// A sub handed out by a gift (the gift itself is its own entry).
    pub gifted: bool,
    pub message: Option<String>,
    pub occurred_at: String,
}

/// What happened from `from` until `until` (open ends when `None`), oldest first; at most the latest `limit`.
pub async fn activity_between(
    pool: &SqlitePool,
    from: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    limit: u32,
) -> Result<Vec<ActivityEntry>, sqlx::Error> {
    sqlx::query_as(
        "SELECT twitch_message_id AS alert_id, kind, user_name, tier, months, count, bits, viewers, gifted, \
         message, occurred_at FROM \
         (SELECT * FROM activity_events WHERE (?1 IS NULL OR occurred_at >= ?1) AND (?2 IS NULL OR occurred_at < ?2) \
          ORDER BY occurred_at DESC LIMIT ?3) ORDER BY occurred_at",
    )
    .bind(from.map(time_text))
    .bind(until.map(time_text))
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Counts what is stored for `session_id` (simulated events never are).
pub async fn session_counts(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<SessionCounts, sqlx::Error> {
    let (follows, subs, bits, raids): (u32, u32, u32, u32) = sqlx::query_as(
        "SELECT COALESCE(SUM(kind = 'follow'), 0), \
         COALESCE(SUM(kind IN ('subscribe', 'resubscribe')), 0), \
         COALESCE(SUM(CASE WHEN kind = 'cheer' THEN bits END), 0), \
         COALESCE(SUM(kind = 'raid'), 0) \
         FROM activity_events WHERE session_id = ?",
    )
    .bind(session_id)
    .fetch_one(pool)
    .await?;
    Ok(SessionCounts {
        follows,
        subs,
        bits,
        raids,
    })
}

impl ActivityRepository for SqliteActivity {
    async fn insert(&self, r: &Record) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            "INSERT INTO activity_events (id, twitch_message_id, kind, user_id, user_login, user_name, tier, \
             months, count, bits, viewers, gifted, message, session_id, occurred_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT (twitch_message_id) DO NOTHING",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(&r.twitch_message_id)
        .bind(r.kind)
        .bind(&r.user_id)
        .bind(&r.user_login)
        .bind(&r.user_name)
        .bind(r.tier)
        .bind(r.months)
        .bind(r.count)
        .bind(r.bits)
        .bind(r.viewers)
        .bind(r.gifted)
        .bind(&r.message)
        .bind(&r.session_id)
        .bind(time_text(r.occurred_at))
        .execute(&self.pool)
        .await
        .map_err(storage)?;
        Ok(result.rows_affected() == 1)
    }

    async fn delete_before(&self, cutoff: DateTime<Utc>) -> Result<u64, RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        let deleted = self
            .delete_all(&mut tx, &time_text(cutoff))
            .await
            .map_err(storage)?;
        tx.commit().await.map_err(storage)?;
        Ok(deleted)
    }

    async fn delete_before_if_saved(
        &self,
        cutoff: Option<DateTime<Utc>>,
        save: impl FnOnce() -> bool + Send,
    ) -> Result<Option<u64>, RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        let deleted = match cutoff {
            Some(cutoff) => self
                .delete_all(&mut tx, &time_text(cutoff))
                .await
                .map_err(storage)?,
            None => 0,
        };
        if !save() {
            tx.rollback().await.map_err(storage)?;
            return Ok(None);
        }
        tx.commit().await.map_err(storage)?;
        Ok(Some(deleted))
    }

    async fn count_before(&self, cutoff: DateTime<Utc>) -> Result<u64, RepositoryError> {
        let cutoff = time_text(cutoff);
        let mut conn = self.pool.acquire().await.map_err(storage)?;
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM activity_events WHERE occurred_at < ?")
                .bind(&cutoff)
                .fetch_one(&mut *conn)
                .await
                .map_err(storage)?;
        let mut count: u64 = n.try_into().unwrap_or(0);
        for table in &self.others {
            count += table
                .count_before(&mut conn, &cutoff)
                .await
                .map_err(storage)?;
        }
        Ok(count)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    pub(crate) async fn pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        MIGRATOR.run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn session_counts_add_up_one_session_only() {
        let pool = pool().await;
        assert_eq!(
            session_counts(&pool, "s1").await.unwrap(),
            SessionCounts::default()
        );
        let rows: [(&str, Option<u32>, i64, &str); 8] = [
            ("follow", None, 0, "s1"),
            ("follow", None, 0, "s1"),
            ("subscribe", None, 0, "s1"),
            ("resubscribe", None, 0, "s1"),
            ("gift", None, 0, "s1"),
            ("subscribe", None, 1, "s1"),
            ("cheer", Some(150), 0, "s1"),
            ("raid", None, 0, "s2"),
        ];
        for (i, (kind, bits, gifted, session)) in rows.into_iter().enumerate() {
            sqlx::query(
                "INSERT INTO activity_events (id, twitch_message_id, kind, bits, gifted, session_id, occurred_at) \
                 VALUES (?, ?, ?, ?, ?, ?, '2026-11-19T10:00:00.000000Z')",
            )
            .bind(i.to_string())
            .bind(format!("m{i}"))
            .bind(kind)
            .bind(bits)
            .bind(gifted)
            .bind(session)
            .execute(&pool)
            .await
            .unwrap();
        }
        assert_eq!(
            session_counts(&pool, "s1").await.unwrap(),
            SessionCounts {
                follows: 2,
                subs: 3,
                bits: 150,
                raids: 0,
            }
        );
        assert_eq!(session_counts(&pool, "s2").await.unwrap().raids, 1);
    }

    /// Another module's table, as a test stand-in.
    struct Other;

    impl RecordTable for Other {
        fn delete_before<'c>(
            &'c self,
            conn: &'c mut SqliteConnection,
            cutoff: &'c str,
        ) -> toolkit_core::records::RecordFuture<'c> {
            Box::pin(async move {
                Ok(sqlx::query("DELETE FROM other WHERE at < ?")
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
        ) -> toolkit_core::records::RecordFuture<'c> {
            Box::pin(async move {
                let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM other WHERE at < ?")
                    .bind(cutoff)
                    .fetch_one(conn)
                    .await?;
                Ok(n as u64)
            })
        }
    }

    #[tokio::test]
    async fn the_retention_covers_other_modules_tables_in_the_same_transaction() {
        let pool = pool().await;
        sqlx::query("CREATE TABLE other (at TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        for at in ["2026-11-01T00:00:00.000000Z", "2026-11-19T00:00:00.000000Z"] {
            sqlx::query("INSERT INTO other (at) VALUES (?)")
                .bind(at)
                .execute(&pool)
                .await
                .unwrap();
        }
        let repo = SqliteActivity::new(pool.clone(), vec![Arc::new(Other)]);
        let cutoff = DateTime::parse_from_rfc3339("2026-11-10T00:00:00Z")
            .unwrap()
            .to_utc();
        assert_eq!(repo.count_before(cutoff).await.unwrap(), 1);
        assert_eq!(
            repo.delete_before_if_saved(Some(cutoff), || false)
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            repo.count_before(cutoff).await.unwrap(),
            1,
            "a failed save deletes nothing anywhere"
        );
        assert_eq!(
            repo.delete_before_if_saved(Some(cutoff), || true)
                .await
                .unwrap(),
            Some(1)
        );
        assert_eq!(repo.count_before(cutoff).await.unwrap(), 0);
    }
}
