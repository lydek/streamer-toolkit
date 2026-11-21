//! Counters in SQLite (`counter_counters`).

use sqlx::{SqlitePool, migrate::Migrator};

use crate::{
    application::{CounterRepository, RepositoryError},
    domain::Counter,
};

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
}
