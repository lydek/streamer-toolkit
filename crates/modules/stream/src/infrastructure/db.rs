//! Stream sessions in SQLite (`stream_sessions`).

use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::{SqlitePool, migrate::Migrator};

use crate::{
    application::{RepositoryError, SessionRepository},
    domain::Session,
};

pub static MIGRATOR: Migrator = sqlx::migrate!();

pub(crate) struct SqliteSessions {
    pool: SqlitePool,
}

impl SqliteSessions {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    id: String,
    twitch_stream_id: String,
    started_at: String,
    ended_at: Option<String>,
}

impl TryFrom<SessionRow> for Session {
    type Error = RepositoryError;

    fn try_from(r: SessionRow) -> Result<Self, RepositoryError> {
        Ok(Session {
            id: r.id,
            twitch_id: r.twitch_stream_id,
            started_at: parse_time(&r.started_at)?,
            ended_at: r.ended_at.as_deref().map(parse_time).transpose()?,
        })
    }
}

fn storage(e: impl std::fmt::Display) -> RepositoryError {
    RepositoryError(e.to_string())
}

/// Fixed width, so text order is time order.
fn time_text(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Micros, true)
}

fn parse_time(s: &str) -> Result<DateTime<Utc>, RepositoryError> {
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.to_utc())
        .map_err(storage)
}

impl SessionRepository for SqliteSessions {
    async fn latest(&self) -> Result<Option<Session>, RepositoryError> {
        sqlx::query_as::<_, SessionRow>(
            "SELECT id, twitch_stream_id, started_at, ended_at FROM stream_sessions \
             ORDER BY started_at DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(storage)?
        .map(Session::try_from)
        .transpose()
    }

    async fn open(
        &self,
        twitch_id: &str,
        started_at: DateTime<Utc>,
    ) -> Result<Session, RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        // A stream whose end was missed (e.g. the toolkit was closed through it) ended before this one started; a
        // stream that started later is never ended by an older one.
        sqlx::query(
            "UPDATE stream_sessions SET ended_at = ?1 \
             WHERE ended_at IS NULL AND twitch_stream_id <> ?2 AND started_at <= ?1",
        )
        .bind(time_text(started_at))
        .bind(twitch_id)
        .execute(&mut *tx)
        .await
        .map_err(storage)?;
        let row = sqlx::query_as::<_, SessionRow>(
            "INSERT INTO stream_sessions (id, twitch_stream_id, started_at) VALUES (?, ?, ?) \
             ON CONFLICT (twitch_stream_id) DO UPDATE SET ended_at = NULL \
             RETURNING id, twitch_stream_id, started_at, ended_at",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(twitch_id)
        .bind(time_text(started_at))
        .fetch_one(&mut *tx)
        .await
        .map_err(storage)?;
        tx.commit().await.map_err(storage)?;
        row.try_into()
    }

    async fn end(&self, id: &str, at: DateTime<Utc>) -> Result<(), RepositoryError> {
        sqlx::query("UPDATE stream_sessions SET ended_at = ? WHERE id = ?")
            .bind(time_text(at))
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(storage)?;
        Ok(())
    }

    async fn raise_peak(&self, id: &str, viewers: u32) -> Result<(), RepositoryError> {
        sqlx::query(
            "UPDATE stream_sessions SET peak_viewers = MAX(COALESCE(peak_viewers, 0), ?) WHERE id = ?",
        )
        .bind(viewers)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(storage)?;
        Ok(())
    }
}

/// The session that started last, live or ended, as the host's stream summary shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestSession {
    pub id: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub peak_viewers: Option<u32>,
}

/// The session that started last; `None` before the first stream.
pub async fn latest_session(pool: &SqlitePool) -> Result<Option<LatestSession>, sqlx::Error> {
    let row: Option<(String, String, Option<String>, Option<u32>)> = sqlx::query_as(
        "SELECT id, started_at, ended_at, peak_viewers FROM stream_sessions \
         ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    let parse = |s: &str| {
        DateTime::parse_from_rfc3339(s)
            .map(|t| t.to_utc())
            .map_err(|e| sqlx::Error::Decode(e.into()))
    };
    row.map(|(id, started_at, ended_at, peak_viewers)| {
        Ok(LatestSession {
            id,
            started_at: parse(&started_at)?,
            ended_at: ended_at.as_deref().map(parse).transpose()?,
            peak_viewers,
        })
    })
    .transpose()
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
    async fn opening_the_same_stream_again_reopens_its_session() {
        let repo = SqliteSessions::new(pool().await);
        let start = parse_time("2026-11-19T10:00:00Z").unwrap();
        let first = repo.open("9001", start).await.unwrap();
        repo.end(&first.id, start + chrono::TimeDelta::hours(2))
            .await
            .unwrap();
        assert!(repo.latest().await.unwrap().unwrap().ended_at.is_some());
        let again = repo.open("9001", start).await.unwrap();
        assert_eq!(again.id, first.id);
        assert_eq!(again.ended_at, None);
        assert_eq!(again.started_at, start);
    }

    #[tokio::test]
    async fn the_peak_keeps_the_most_viewers_and_the_latest_session_shows_it() {
        let pool = pool().await;
        let repo = SqliteSessions::new(pool.clone());
        assert_eq!(latest_session(&pool).await.unwrap(), None);
        let start = parse_time("2026-11-19T10:00:00Z").unwrap();
        let s = repo.open("9001", start).await.unwrap();
        assert_eq!(
            latest_session(&pool).await.unwrap().unwrap().peak_viewers,
            None
        );
        for viewers in [40, 58, 51] {
            repo.raise_peak(&s.id, viewers).await.unwrap();
        }
        let end = start + chrono::TimeDelta::hours(2);
        repo.end(&s.id, end).await.unwrap();
        assert_eq!(
            latest_session(&pool).await.unwrap(),
            Some(LatestSession {
                id: s.id,
                started_at: start,
                ended_at: Some(end),
                peak_viewers: Some(58),
            })
        );
    }

    #[tokio::test]
    async fn an_older_stream_never_ends_a_newer_one() {
        let pool = pool().await;
        let repo = SqliteSessions::new(pool.clone());
        repo.open("B", parse_time("2026-11-19T11:00:00Z").unwrap())
            .await
            .unwrap();
        repo.open("A", parse_time("2026-11-19T10:00:00Z").unwrap())
            .await
            .unwrap();
        let b_end: Option<String> =
            sqlx::query_scalar("SELECT ended_at FROM stream_sessions WHERE twitch_stream_id = 'B'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(b_end, None);
    }

    #[tokio::test]
    async fn a_new_stream_ends_one_whose_end_was_missed() {
        let pool = pool().await;
        let repo = SqliteSessions::new(pool.clone());
        let a_start = parse_time("2026-11-19T10:00:00Z").unwrap();
        let b_start = parse_time("2026-11-20T10:00:00Z").unwrap();
        repo.open("A", a_start).await.unwrap();
        // Closed during A, opened again during B: only a check for B arrives.
        repo.open("B", b_start).await.unwrap();
        let open: Vec<String> = sqlx::query_scalar(
            "SELECT twitch_stream_id FROM stream_sessions WHERE ended_at IS NULL",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(open, ["B"]);
        let a_end: String =
            sqlx::query_scalar("SELECT ended_at FROM stream_sessions WHERE twitch_stream_id = 'A'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(parse_time(&a_end).unwrap(), b_start);
    }
}
