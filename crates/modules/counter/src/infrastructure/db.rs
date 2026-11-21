//! Counters in SQLite (`counter_counters`).

use sqlx::{SqlitePool, migrate::Migrator};

use crate::{
    application::{CounterRepository, RepositoryError},
    domain::{
        Counter, ViewerCount, ViewerCounter,
        marathon::{Marathon, MarathonOptions},
    },
};

/// The most viewers a search returns.
const SEARCH_LIMIT: u32 = 20;

pub static MIGRATOR: Migrator = sqlx::migrate!();

pub struct SqliteCounters {
    pool: SqlitePool,
}

impl SqliteCounters {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

// Thin module: the row is the model.
#[derive(sqlx::FromRow)]
struct Row {
    id: String,
    name: String,
    value: i64,
    per_stream: bool,
    reset_session: Option<String>,
}

/// remaining_ms, anchor_at, paused, session, sub_minutes, bits_minutes, max_hours.
type MarathonRow = (i64, String, bool, Option<String>, i64, i64, i64);

fn storage(e: sqlx::Error) -> RepositoryError {
    RepositoryError(e.to_string())
}

impl CounterRepository for SqliteCounters {
    async fn list(&self) -> Result<Vec<Counter>, RepositoryError> {
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT id, name, value, per_stream, reset_session FROM counter_counters ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(storage)?;
        Ok(rows
            .into_iter()
            .map(|r| Counter {
                id: r.id,
                name: r.name,
                value: r.value,
                per_stream: r.per_stream,
                reset_session: r.reset_session,
            })
            .collect())
    }

    async fn insert(&self, c: &Counter) -> Result<(), RepositoryError> {
        sqlx::query("INSERT INTO counter_counters (id, name, value, per_stream, reset_session) VALUES (?, ?, ?, ?, ?)")
            .bind(&c.id)
            .bind(&c.name)
            .bind(c.value)
            .bind(c.per_stream)
            .bind(&c.reset_session)
            .execute(&self.pool)
            .await
            .map_err(storage)?;
        Ok(())
    }

    async fn update(&self, c: &Counter) -> Result<bool, RepositoryError> {
        let done = sqlx::query(
            "UPDATE counter_counters SET name = ?, per_stream = ?, reset_session = ? WHERE id = ?",
        )
        .bind(&c.name)
        .bind(c.per_stream)
        .bind(&c.reset_session)
        .bind(&c.id)
        .execute(&self.pool)
        .await
        .map_err(storage)?;
        Ok(done.rows_affected() == 1)
    }

    async fn delete(&self, id: &str) -> Result<bool, RepositoryError> {
        let done = sqlx::query("DELETE FROM counter_counters WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(storage)?;
        Ok(done.rows_affected() == 1)
    }

    async fn add(&self, id: &str, delta: i64) -> Result<bool, RepositoryError> {
        let done =
            sqlx::query("UPDATE counter_counters SET value = MAX(value + ?, 0) WHERE id = ?")
                .bind(delta)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(storage)?;
        Ok(done.rows_affected() == 1)
    }

    async fn reset(&self, id: &str) -> Result<bool, RepositoryError> {
        let done = sqlx::query("UPDATE counter_counters SET value = 0 WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(storage)?;
        Ok(done.rows_affected() == 1)
    }

    async fn reset_for_session(&self, session: &str) -> Result<u64, RepositoryError> {
        let done = sqlx::query(
            "UPDATE counter_counters SET value = 0, reset_session = ?1 \
             WHERE per_stream = 1 AND (reset_session IS NULL OR reset_session <> ?1)",
        )
        .bind(session)
        .execute(&self.pool)
        .await
        .map_err(storage)?;
        Ok(done.rows_affected())
    }

    async fn viewer_counters(&self) -> Result<Vec<ViewerCounter>, RepositoryError> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT id, name FROM counter_viewer_counters ORDER BY id")
                .fetch_all(&self.pool)
                .await
                .map_err(storage)?;
        Ok(rows
            .into_iter()
            .map(|(id, name)| ViewerCounter { id, name })
            .collect())
    }

    async fn insert_viewer_counter(&self, c: &ViewerCounter) -> Result<(), RepositoryError> {
        sqlx::query("INSERT INTO counter_viewer_counters (id, name) VALUES (?, ?)")
            .bind(&c.id)
            .bind(&c.name)
            .execute(&self.pool)
            .await
            .map_err(storage)?;
        Ok(())
    }

    async fn rename_viewer_counter(&self, id: &str, name: &str) -> Result<bool, RepositoryError> {
        let done = sqlx::query("UPDATE counter_viewer_counters SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(storage)?;
        Ok(done.rows_affected() == 1)
    }

    async fn delete_viewer_counter(&self, id: &str) -> Result<bool, RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        sqlx::query("DELETE FROM counter_viewer_counts WHERE counter_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(storage)?;
        let done = sqlx::query("DELETE FROM counter_viewer_counters WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(storage)?;
        tx.commit().await.map_err(storage)?;
        Ok(done.rows_affected() == 1)
    }

    async fn add_for_viewer(
        &self,
        counter_id: &str,
        twitch_user_id: &str,
        delta: i64,
    ) -> Result<bool, RepositoryError> {
        let done = sqlx::query(
            "INSERT INTO counter_viewer_counts (counter_id, twitch_user_id, value) \
             SELECT id, ?2, MAX(?3, 0) FROM counter_viewer_counters WHERE id = ?1 \
             ON CONFLICT (counter_id, twitch_user_id) DO UPDATE SET value = MAX(value + ?3, 0)",
        )
        .bind(counter_id)
        .bind(twitch_user_id)
        .bind(delta)
        .execute(&self.pool)
        .await
        .map_err(storage)?;
        Ok(done.rows_affected() == 1)
    }

    async fn viewer_counts(
        &self,
        counter_id: &str,
        limit: Option<u32>,
    ) -> Result<Vec<ViewerCount>, RepositoryError> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT twitch_user_id, value FROM counter_viewer_counts \
             WHERE counter_id = ? AND value > 0 ORDER BY value DESC, twitch_user_id LIMIT ?",
        )
        .bind(counter_id)
        .bind(limit.map_or(-1, i64::from))
        .fetch_all(&self.pool)
        .await
        .map_err(storage)?;
        let ids: Vec<String> = rows.iter().map(|(id, _)| id.clone()).collect();
        let names = viewers::names(&self.pool, &ids).await.map_err(storage)?;
        Ok(rows
            .into_iter()
            .map(|(id, value)| {
                // A viewer missing from the directory still counts; its ID stands in for its names.
                let name = names.get(&id);
                ViewerCount {
                    login: name.map_or_else(|| id.clone(), |n| n.login.clone()),
                    display_name: name.map_or_else(|| id.clone(), |n| n.display_name.clone()),
                    twitch_user_id: id,
                    value,
                }
            })
            .collect())
    }

    async fn viewer_total(&self, counter_id: &str) -> Result<i64, RepositoryError> {
        sqlx::query_scalar(
            "SELECT COALESCE(SUM(value), 0) FROM counter_viewer_counts WHERE counter_id = ?",
        )
        .bind(counter_id)
        .fetch_one(&self.pool)
        .await
        .map_err(storage)
    }

    async fn search_viewers(&self, query: &str) -> Result<Vec<ViewerCount>, RepositoryError> {
        let found = viewers::search(&self.pool, query, SEARCH_LIMIT)
            .await
            .map_err(storage)?;
        Ok(found
            .into_iter()
            .map(|v| ViewerCount {
                twitch_user_id: v.twitch_user_id,
                login: v.login,
                display_name: v.display_name,
                value: 0,
            })
            .collect())
    }

    async fn viewer_known(&self, twitch_user_id: &str) -> Result<bool, RepositoryError> {
        let found = viewers::names(&self.pool, &[twitch_user_id.to_owned()])
            .await
            .map_err(storage)?;
        Ok(!found.is_empty())
    }

    async fn load_marathon(&self) -> Result<Option<Marathon>, RepositoryError> {
        let row: Option<MarathonRow> = sqlx::query_as(
            "SELECT remaining_ms, anchor_at, paused, session, sub_minutes, bits_minutes, max_hours \
             FROM counter_marathon WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(storage)?;
        let Some((remaining_ms, anchor, paused, session, sub, bits, hours)) = row else {
            return Ok(None);
        };
        let number = |n: i64| u32::try_from(n).map_err(|e| RepositoryError(e.to_string()));
        Ok(Some(Marathon {
            remaining_ms,
            anchor: chrono::DateTime::parse_from_rfc3339(&anchor)
                .map_err(|e| RepositoryError(e.to_string()))?
                .to_utc(),
            paused,
            session,
            options: MarathonOptions {
                sub_minutes: number(sub)?,
                bits_minutes: number(bits)?,
                max_hours: number(hours)?,
            },
        }))
    }

    async fn save_marathon(&self, m: &Marathon) -> Result<(), RepositoryError> {
        sqlx::query(
            "INSERT INTO counter_marathon \
             (id, remaining_ms, anchor_at, paused, session, sub_minutes, bits_minutes, max_hours) \
             VALUES (1, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (id) DO UPDATE SET remaining_ms = excluded.remaining_ms, anchor_at = excluded.anchor_at, \
             paused = excluded.paused, session = excluded.session, sub_minutes = excluded.sub_minutes, \
             bits_minutes = excluded.bits_minutes, max_hours = excluded.max_hours",
        )
        .bind(m.remaining_ms)
        .bind(m.anchor.to_rfc3339_opts(chrono::SecondsFormat::Micros, true))
        .bind(m.paused)
        .bind(&m.session)
        .bind(m.options.sub_minutes)
        .bind(m.options.bits_minutes)
        .bind(m.options.max_hours)
        .execute(&self.pool)
        .await
        .map_err(storage)?;
        Ok(())
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
        // The viewer directory shares the database; the Host merges every module's migrations the same way.
        let mut all: Vec<_> = MIGRATOR
            .iter()
            .chain(viewers::MIGRATOR.iter())
            .cloned()
            .collect();
        all.sort_by_key(|m| m.version);
        Migrator::with_migrations(all).run(&pool).await.unwrap();
        pool
    }
}
