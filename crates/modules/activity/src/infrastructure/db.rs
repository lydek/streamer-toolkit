//! Activity records in SQLite (`activity_events`).

use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::{SqlitePool, migrate::Migrator};

use crate::{
    application::{ActivityRepository, RepositoryError},
    domain::Record,
};

pub static MIGRATOR: Migrator = sqlx::migrate!();

pub(crate) struct SqliteActivity {
    pool: SqlitePool,
}

impl SqliteActivity {
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
        let result = sqlx::query("DELETE FROM activity_events WHERE occurred_at < ?")
            .bind(time_text(cutoff))
            .execute(&self.pool)
            .await
            .map_err(storage)?;
        Ok(result.rows_affected())
    }

    async fn delete_before_if_saved(
        &self,
        cutoff: Option<DateTime<Utc>>,
        save: impl FnOnce() -> bool + Send,
    ) -> Result<Option<u64>, RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        let deleted = match cutoff {
            Some(cutoff) => sqlx::query("DELETE FROM activity_events WHERE occurred_at < ?")
                .bind(time_text(cutoff))
                .execute(&mut *tx)
                .await
                .map_err(storage)?
                .rows_affected(),
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
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM activity_events WHERE occurred_at < ?")
                .bind(time_text(cutoff))
                .fetch_one(&self.pool)
                .await
                .map_err(storage)?;
        Ok(n.try_into().unwrap_or(0))
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
