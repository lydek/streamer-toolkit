//! Activity records in SQLite (`activity_events`).

use std::sync::Arc;

use sqlx::{SqliteConnection, SqlitePool, migrate::Migrator};
use toolkit_core::records::RecordTable;

use crate::{
    application::{ActivityRepository, RepositoryError},
    domain::Record,
};
use toolkit_core::time::{self, Timestamp};

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
fn time_text(t: Timestamp) -> String {
    time::micros(t)
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

/// What happened from `from` until `until` (open ends when `None`), oldest first; at most the latest `limit`. The subs a
/// gift hands out are left out (the gift stands for them), so a big gift never pushes the rest past the limit.
pub async fn activity_between(
    pool: &SqlitePool,
    from: Option<Timestamp>,
    until: Option<Timestamp>,
    limit: u32,
) -> Result<Vec<ActivityEntry>, sqlx::Error> {
    sqlx::query_as(
        "SELECT twitch_message_id AS alert_id, kind, user_name, tier, months, count, bits, viewers, gifted, \
         message, occurred_at FROM \
         (SELECT * FROM activity_events WHERE gifted = 0 AND (?1 IS NULL OR occurred_at >= ?1) \
          AND (?2 IS NULL OR occurred_at < ?2) ORDER BY occurred_at DESC, id DESC LIMIT ?3) ORDER BY occurred_at, id",
    )
    .bind(from.map(time_text))
    .bind(until.map(time_text))
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// What a leaderboard ranks from the activity records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ranking {
    /// Bits cheered.
    Bits,
    /// Subs gifted (the gifts, not the subs they hand out).
    Gifts,
}

/// The top `limit` viewers from `from` until `until` (open ends when `None`), most first: name and amount. Logins in
/// `exclude` (lowercase) and anonymous gifts or cheers are left out; simulated events are never stored. A viewer goes
/// by the login and name of their latest entry counted, so a rename shows.
pub async fn ranking(
    pool: &SqlitePool,
    by: Ranking,
    from: Option<Timestamp>,
    until: Option<Timestamp>,
    exclude: &[&str],
    limit: u32,
) -> Result<Vec<(String, u64)>, sqlx::Error> {
    // Static queries: one per ranking.
    let sql = match by {
        Ranking::Bits => {
            "WITH ranked AS (SELECT user_id, user_login, user_name, bits AS amount, occurred_at, \
             ROW_NUMBER() OVER (PARTITION BY user_id ORDER BY occurred_at DESC, id DESC) AS latest \
             FROM activity_events WHERE kind = 'cheer' AND user_id IS NOT NULL \
             AND (?1 IS NULL OR occurred_at >= ?1) AND (?2 IS NULL OR occurred_at < ?2)) \
             SELECT LOWER(COALESCE(MAX(CASE WHEN latest = 1 THEN user_login END), '')), \
             COALESCE(MAX(CASE WHEN latest = 1 THEN user_name END), ''), SUM(COALESCE(amount, 0)) AS total \
             FROM ranked GROUP BY user_id ORDER BY total DESC, MIN(occurred_at)"
        }
        Ranking::Gifts => {
            "WITH ranked AS (SELECT user_id, user_login, user_name, count AS amount, occurred_at, \
             ROW_NUMBER() OVER (PARTITION BY user_id ORDER BY occurred_at DESC, id DESC) AS latest \
             FROM activity_events WHERE kind = 'gift' AND user_id IS NOT NULL \
             AND (?1 IS NULL OR occurred_at >= ?1) AND (?2 IS NULL OR occurred_at < ?2)) \
             SELECT LOWER(COALESCE(MAX(CASE WHEN latest = 1 THEN user_login END), '')), \
             COALESCE(MAX(CASE WHEN latest = 1 THEN user_name END), ''), SUM(COALESCE(amount, 0)) AS total \
             FROM ranked GROUP BY user_id ORDER BY total DESC, MIN(occurred_at)"
        }
    };
    let rows: Vec<(String, String, i64)> = sqlx::query_as(sql)
        .bind(from.map(time_text))
        .bind(until.map(time_text))
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .filter(|(login, _, total)| *total > 0 && !exclude.contains(&login.as_str()))
        .take(limit as usize)
        .map(|(_, name, total)| (name, u64::try_from(total).unwrap_or(0)))
        .collect())
}

/// What `session_id`'s stats count, one entry each, oldest first: follows, subs and resubs (each sub a gift hands
/// out included; the gift itself is left out, as the stats do), cheers and raids.
pub async fn session_entries(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<Vec<ActivityEntry>, sqlx::Error> {
    sqlx::query_as(
        "SELECT twitch_message_id AS alert_id, kind, user_name, tier, months, count, bits, viewers, gifted, \
         message, occurred_at FROM activity_events WHERE session_id = ? AND kind <> 'gift' ORDER BY occurred_at, id",
    )
    .bind(session_id)
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

    async fn delete_before(&self, cutoff: Timestamp) -> Result<u64, RepositoryError> {
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
        cutoff: Option<Timestamp>,
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

    async fn count_before(&self, cutoff: Timestamp) -> Result<u64, RepositoryError> {
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

    #[tokio::test]
    async fn a_big_gift_never_pushes_older_activity_past_the_limit() {
        let pool = pool().await;
        // A follow, then a gift of 50 and the 50 subs it hands out.
        sqlx::query(
            "WITH RECURSIVE i(n) AS (SELECT 0 UNION ALL SELECT n + 1 FROM i WHERE n < 51) \
             INSERT INTO activity_events (id, twitch_message_id, kind, count, gifted, occurred_at) \
             SELECT n, 'm' || n, CASE n WHEN 0 THEN 'follow' WHEN 1 THEN 'gift' ELSE 'subscribe' END, \
             CASE n WHEN 1 THEN 50 END, n > 1, strftime('%Y-%m-%dT%H:%M:%S.000000Z', '2026-11-19', '+' || n || ' seconds') \
             FROM i",
        )
        .execute(&pool)
        .await
        .unwrap();
        let kinds: Vec<String> = activity_between(&pool, None, None, 50)
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.kind)
            .collect();
        assert_eq!(kinds, ["follow", "gift"]);
    }

    #[tokio::test]
    async fn activity_from_the_same_instant_keeps_the_order_it_arrived_in() {
        let pool = pool().await;
        // UUIDv7 IDs grow in insert order; the kinds tell the rows apart.
        for (id, kind) in [("01", "raid"), ("02", "follow"), ("03", "cheer")] {
            sqlx::query(
                "INSERT INTO activity_events (id, twitch_message_id, kind, session_id, occurred_at) \
                 VALUES (?, ?, ?, 's1', '2026-11-19T10:00:00.000000Z')",
            )
            .bind(id)
            .bind(format!("m{id}"))
            .bind(kind)
            .execute(&pool)
            .await
            .unwrap();
        }
        let kinds =
            |entries: Vec<ActivityEntry>| entries.into_iter().map(|e| e.kind).collect::<Vec<_>>();
        assert_eq!(
            kinds(activity_between(&pool, None, None, 10).await.unwrap()),
            ["raid", "follow", "cheer"]
        );
        assert_eq!(
            kinds(activity_between(&pool, None, None, 2).await.unwrap()),
            ["follow", "cheer"]
        );
        assert_eq!(
            kinds(session_entries(&pool, "s1").await.unwrap()),
            ["raid", "follow", "cheer"]
        );
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
        let cutoff = "2026-11-10T00:00:00Z".parse::<Timestamp>().unwrap();
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

    #[tokio::test]
    async fn rankings_go_by_the_latest_login_and_name_in_the_time_asked() {
        let pool = pool().await;
        // u1: an older name that sorts last, then two in the same instant (the later stored wins), then one after the
        // time asked; u2 gave more.
        let rows = [
            ("01", "u1", "zoe", "Zoe", 100, "2026-11-19T10:00:00.000000Z"),
            ("02", "u1", "amy", "Amy", 50, "2026-11-19T10:05:00.000000Z"),
            ("03", "u1", "ann", "Ann", 10, "2026-11-19T10:05:00.000000Z"),
            ("04", "u1", "bea", "Bea", 999, "2026-11-19T11:00:00.000000Z"),
            ("05", "u2", "kai", "Kai", 500, "2026-11-19T10:01:00.000000Z"),
        ];
        for (id, user, login, name, amount, at) in rows {
            for (kind, bits, count) in [("cheer", Some(amount), None), ("gift", None, Some(amount))]
            {
                sqlx::query(
                    "INSERT INTO activity_events (id, twitch_message_id, kind, user_id, user_login, user_name, bits, \
                     count, occurred_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(format!("{id}{kind}"))
                .bind(format!("m{id}{kind}"))
                .bind(kind)
                .bind(user)
                .bind(login)
                .bind(name)
                .bind(bits)
                .bind(count)
                .bind(at)
                .execute(&pool)
                .await
                .unwrap();
            }
        }
        let at = |t: &str| t.parse::<Timestamp>().unwrap();
        let (from, until) = (
            Some(at("2026-11-19T09:00:00Z")),
            Some(at("2026-11-19T10:30:00Z")),
        );
        for by in [Ranking::Bits, Ranking::Gifts] {
            assert_eq!(
                ranking(&pool, by, from, until, &[], 10).await.unwrap(),
                [("Kai".to_owned(), 500), ("Ann".to_owned(), 160)],
                "{by:?}"
            );
            assert_eq!(
                ranking(&pool, by, from, until, &["ann"], 10).await.unwrap(),
                [("Kai".to_owned(), 500)],
                "{by:?}"
            );
        }
    }
}
