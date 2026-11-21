//! Screen tools in SQLite (`overlays_break_card`).

use chrono::{DateTime, Utc};
use sqlx::{SqlitePool, migrate::Migrator};

use crate::{
    application::{OverlayRepository, RepositoryError},
    domain::{BreakCard, BreakSettings, CountdownMode},
};

pub static MIGRATOR: Migrator = sqlx::migrate!();

pub struct SqliteOverlays {
    pool: SqlitePool,
}

impl SqliteOverlays {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn storage(e: impl ToString) -> RepositoryError {
    RepositoryError(e.to_string())
}

/// text, end_text, mode, minutes, until_time, shown, ends_at.
type BreakRow = (String, String, String, i64, String, bool, Option<String>);

impl OverlayRepository for SqliteOverlays {
    async fn break_card(&self) -> Result<Option<BreakCard>, RepositoryError> {
        let row: Option<BreakRow> = sqlx::query_as(
            "SELECT text, end_text, mode, minutes, until_time, shown, ends_at FROM overlays_break_card",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(storage)?;
        let Some((text, end_text, mode, minutes, until_time, shown, ends_at)) = row else {
            return Ok(None);
        };
        let ends_at = ends_at
            .map(|t| DateTime::parse_from_rfc3339(&t).map(|t| t.to_utc()))
            .transpose()
            .map_err(storage)?;
        Ok(Some(BreakCard {
            settings: BreakSettings {
                text,
                end_text,
                mode: CountdownMode::parse(&mode).ok_or_else(|| storage("unknown mode"))?,
                minutes: u32::try_from(minutes).map_err(storage)?,
                until_time,
            },
            shown,
            ends_at,
        }))
    }

    async fn save_break_card(&self, card: &BreakCard) -> Result<(), RepositoryError> {
        let s = &card.settings;
        sqlx::query(
            "INSERT INTO overlays_break_card (id, text, end_text, mode, minutes, until_time, shown, ends_at)
             VALUES (1, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET text = excluded.text, end_text = excluded.end_text,
               mode = excluded.mode, minutes = excluded.minutes, until_time = excluded.until_time,
               shown = excluded.shown, ends_at = excluded.ends_at",
        )
        .bind(&s.text)
        .bind(&s.end_text)
        .bind(s.mode.as_str())
        .bind(i64::from(s.minutes))
        .bind(&s.until_time)
        .bind(card.shown)
        .bind(card.ends_at.map(rfc3339))
        .execute(&self.pool)
        .await
        .map_err(storage)?;
        Ok(())
    }
}

pub(crate) fn rfc3339(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
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
