//! Screen tools in SQLite (`overlays_break_card`, `overlays_warnings`, `overlays_appearance`, `overlays_media`).

use chrono::{DateTime, Utc};
use sqlx::{SqlitePool, migrate::Migrator};

use crate::{
    application::{OverlayRepository, RepositoryError},
    domain::{
        BreakCard, BreakSettings, CountdownMode,
        appearance::{Appearance, Theme},
        media::{MediaEntry, MediaFormat, MediaRule, Position},
        warnings::WarningSettings,
    },
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

    async fn warning_settings(&self) -> Result<Option<WarningSettings>, RepositoryError> {
        let row: Option<(bool, bool, bool, i64, Option<String>)> = sqlx::query_as(
            "SELECT chat_overlay, twitch, mic, mic_seconds, mic_input FROM overlays_warnings",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(storage)?;
        row.map(|(chat_overlay, twitch, mic, mic_seconds, mic_input)| {
            Ok(WarningSettings {
                chat_overlay,
                twitch,
                mic,
                mic_seconds: u32::try_from(mic_seconds).map_err(storage)?,
                mic_input,
            })
        })
        .transpose()
    }

    async fn save_warning_settings(&self, s: &WarningSettings) -> Result<(), RepositoryError> {
        sqlx::query(
            "INSERT INTO overlays_warnings (id, chat_overlay, twitch, mic, mic_seconds, mic_input)
             VALUES (1, ?, ?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET chat_overlay = excluded.chat_overlay, twitch = excluded.twitch,
               mic = excluded.mic, mic_seconds = excluded.mic_seconds, mic_input = excluded.mic_input",
        )
        .bind(s.chat_overlay)
        .bind(s.twitch)
        .bind(s.mic)
        .bind(i64::from(s.mic_seconds))
        .bind(&s.mic_input)
        .execute(&self.pool)
        .await
        .map_err(storage)?;
        Ok(())
    }

    async fn appearance(&self) -> Result<Option<Appearance>, RepositoryError> {
        let row: Option<(String, i64)> =
            sqlx::query_as("SELECT theme, text_size FROM overlays_appearance")
                .fetch_optional(&self.pool)
                .await
                .map_err(storage)?;
        row.map(|(theme, text_size)| {
            Ok(Appearance {
                theme: Theme::parse(&theme).ok_or_else(|| storage("unknown theme"))?,
                text_size: u16::try_from(text_size).map_err(storage)?,
            })
        })
        .transpose()
    }

    async fn save_appearance(&self, a: Appearance) -> Result<(), RepositoryError> {
        sqlx::query(
            "INSERT INTO overlays_appearance (id, theme, text_size) VALUES (1, ?, ?)
             ON CONFLICT (id) DO UPDATE SET theme = excluded.theme, text_size = excluded.text_size",
        )
        .bind(a.theme.as_str())
        .bind(i64::from(a.text_size))
        .execute(&self.pool)
        .await
        .map_err(storage)?;
        Ok(())
    }

    async fn media(&self) -> Result<Vec<MediaEntry>, RepositoryError> {
        let rows: Vec<MediaRow> =
            sqlx::query_as("SELECT id, title, format, duration_ms FROM overlays_media ORDER BY id")
                .fetch_all(&self.pool)
                .await
                .map_err(storage)?;
        rows.into_iter().map(media_entry).collect()
    }

    async fn media_entry(&self, id: &str) -> Result<Option<MediaEntry>, RepositoryError> {
        let row: Option<MediaRow> = sqlx::query_as(
            "SELECT id, title, format, duration_ms FROM overlays_media WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(storage)?;
        row.map(media_entry).transpose()
    }

    async fn insert_media(&self, e: &MediaEntry) -> Result<(), RepositoryError> {
        sqlx::query(
            "INSERT INTO overlays_media (id, title, format, duration_ms) VALUES (?, ?, ?, ?)",
        )
        .bind(&e.id)
        .bind(&e.title)
        .bind(e.format.extension())
        .bind(e.duration_ms.map(i64::from))
        .execute(&self.pool)
        .await
        .map_err(storage)?;
        Ok(())
    }

    async fn update_media(&self, e: &MediaEntry) -> Result<(), RepositoryError> {
        sqlx::query("UPDATE overlays_media SET title = ?, duration_ms = ? WHERE id = ?")
            .bind(&e.title)
            .bind(e.duration_ms.map(i64::from))
            .bind(&e.id)
            .execute(&self.pool)
            .await
            .map_err(storage)?;
        Ok(())
    }

    async fn delete_media(&self, id: &str) -> Result<(), RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        sqlx::query("DELETE FROM overlays_media_rules WHERE media_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(storage)?;
        sqlx::query("DELETE FROM overlays_media WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(storage)?;
        tx.commit().await.map_err(storage)
    }

    async fn media_rules(&self) -> Result<Vec<MediaRule>, RepositoryError> {
        let rows: Vec<(String, String, String, Option<i64>)> = sqlx::query_as(
            "SELECT reward_id, media_id, position, volume FROM overlays_media_rules ORDER BY reward_id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(storage)?;
        rows.into_iter()
            .map(|(reward_id, media_id, position, volume)| {
                Ok(MediaRule {
                    reward_id,
                    media_id,
                    position: Position::parse(&position)
                        .ok_or_else(|| storage("unknown position"))?,
                    volume: volume.map(u8::try_from).transpose().map_err(storage)?,
                })
            })
            .collect()
    }

    async fn save_media_rule(&self, r: &MediaRule) -> Result<(), RepositoryError> {
        sqlx::query(
            "INSERT INTO overlays_media_rules (reward_id, media_id, position, volume) VALUES (?, ?, ?, ?)
             ON CONFLICT (reward_id) DO UPDATE SET media_id = excluded.media_id,
               position = excluded.position, volume = excluded.volume",
        )
        .bind(&r.reward_id)
        .bind(&r.media_id)
        .bind(r.position.as_str())
        .bind(r.volume.map(i64::from))
        .execute(&self.pool)
        .await
        .map_err(storage)?;
        Ok(())
    }

    async fn delete_media_rule(&self, reward_id: &str) -> Result<bool, RepositoryError> {
        let done = sqlx::query("DELETE FROM overlays_media_rules WHERE reward_id = ?")
            .bind(reward_id)
            .execute(&self.pool)
            .await
            .map_err(storage)?;
        Ok(done.rows_affected() > 0)
    }
}

/// id, title, format, duration_ms.
type MediaRow = (String, String, String, Option<i64>);

fn media_entry((id, title, format, duration_ms): MediaRow) -> Result<MediaEntry, RepositoryError> {
    Ok(MediaEntry {
        id,
        title,
        format: MediaFormat::from_extension(&format)
            .ok_or_else(|| storage("unknown media format"))?,
        duration_ms: duration_ms
            .map(u32::try_from)
            .transpose()
            .map_err(storage)?,
    })
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
