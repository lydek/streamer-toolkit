//! The song list in SQLite (`songs_songs`, `songs_categories`).

use std::collections::HashMap;

use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::{SqlitePool, migrate::Migrator};

use crate::{
    application::{RepositoryError, SongRepository},
    domain::{Category, ImportPlan, Song, normalize, song_key},
};

pub static MIGRATOR: Migrator = sqlx::migrate!();

pub struct SqliteSongs {
    pool: SqlitePool,
}

impl SqliteSongs {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn storage(e: sqlx::Error) -> RepositoryError {
    RepositoryError(e.to_string())
}

/// A unique key broke: the caller answers `false` instead of failing.
fn duplicate(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .is_some_and(|d| d.is_unique_violation())
}

pub(crate) fn time(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Micros, true)
}

#[derive(sqlx::FromRow)]
struct SongRow {
    id: String,
    title: String,
    artist: String,
    category_id: Option<String>,
    youtube_url: Option<String>,
    last_sung_at: Option<String>,
    request_count: i64,
}

impl From<SongRow> for Song {
    fn from(r: SongRow) -> Self {
        Song {
            id: r.id,
            title: r.title,
            artist: r.artist,
            category_id: r.category_id,
            youtube_url: r.youtube_url,
            last_sung_at: r
                .last_sung_at
                .and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
                .map(|t| t.to_utc()),
            request_count: u32::try_from(r.request_count).unwrap_or(0),
        }
    }
}

impl SongRepository for SqliteSongs {
    async fn songs(&self) -> Result<Vec<Song>, RepositoryError> {
        let rows: Vec<SongRow> = sqlx::query_as(
            "SELECT id, title, artist, category_id, youtube_url, last_sung_at, request_count \
             FROM songs_songs ORDER BY title COLLATE NOCASE, artist COLLATE NOCASE",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(storage)?;
        Ok(rows.into_iter().map(Song::from).collect())
    }

    async fn categories(&self) -> Result<Vec<Category>, RepositoryError> {
        let rows: Vec<(String, String, bool)> =
            sqlx::query_as("SELECT id, name, requestable FROM songs_categories ORDER BY id")
                .fetch_all(&self.pool)
                .await
                .map_err(storage)?;
        Ok(rows
            .into_iter()
            .map(|(id, name, requestable)| Category {
                id,
                name,
                requestable,
            })
            .collect())
    }

    async fn save_song(&self, s: &Song) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            "INSERT INTO songs_songs (id, title, artist, song_key, category_id, youtube_url, last_sung_at, request_count) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (id) DO UPDATE SET title = excluded.title, artist = excluded.artist, \
             song_key = excluded.song_key, category_id = excluded.category_id, youtube_url = excluded.youtube_url",
        )
        .bind(&s.id)
        .bind(&s.title)
        .bind(&s.artist)
        .bind(song_key(&s.title, &s.artist))
        .bind(&s.category_id)
        .bind(&s.youtube_url)
        .bind(s.last_sung_at.map(time))
        .bind(i64::from(s.request_count))
        .execute(&self.pool)
        .await;
        match result {
            Ok(_) => Ok(true),
            Err(e) if duplicate(&e) => Ok(false),
            Err(e) => Err(storage(e)),
        }
    }

    async fn delete_song(&self, id: &str) -> Result<bool, RepositoryError> {
        let done = sqlx::query("DELETE FROM songs_songs WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(storage)?;
        Ok(done.rows_affected() > 0)
    }

    async fn save_category(&self, c: &Category) -> Result<bool, RepositoryError> {
        let result = sqlx::query(
            "INSERT INTO songs_categories (id, name, name_key, requestable) VALUES (?, ?, ?, ?) \
             ON CONFLICT (id) DO UPDATE SET name = excluded.name, name_key = excluded.name_key, \
             requestable = excluded.requestable",
        )
        .bind(&c.id)
        .bind(&c.name)
        .bind(normalize(&c.name))
        .bind(c.requestable)
        .execute(&self.pool)
        .await;
        match result {
            Ok(_) => Ok(true),
            Err(e) if duplicate(&e) => Ok(false),
            Err(e) => Err(storage(e)),
        }
    }

    async fn delete_category(&self, id: &str) -> Result<bool, RepositoryError> {
        // The foreign key leaves its songs uncategorised (ON DELETE SET NULL).
        let done = sqlx::query("DELETE FROM songs_categories WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(storage)?;
        Ok(done.rows_affected() > 0)
    }

    async fn apply_import(&self, plan: &ImportPlan) -> Result<(), RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        for name in &plan.new_categories {
            sqlx::query("INSERT INTO songs_categories (id, name, name_key, requestable) VALUES (?, ?, ?, 1)")
                .bind(uuid::Uuid::now_v7().to_string())
                .bind(name)
                .bind(normalize(name))
                .execute(&mut *tx)
                .await
                .map_err(storage)?;
        }
        let ids: HashMap<String, String> =
            sqlx::query_as::<_, (String, String)>("SELECT name_key, id FROM songs_categories")
                .fetch_all(&mut *tx)
                .await
                .map_err(storage)?
                .into_iter()
                .collect();
        let category =
            |name: &Option<String>| name.as_ref().and_then(|n| ids.get(&normalize(n)).cloned());
        for s in &plan.add {
            sqlx::query(
                "INSERT INTO songs_songs (id, title, artist, song_key, category_id, youtube_url) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(uuid::Uuid::now_v7().to_string())
            .bind(&s.title)
            .bind(&s.artist)
            .bind(song_key(&s.title, &s.artist))
            .bind(category(&s.category))
            .bind(&s.youtube_url)
            .execute(&mut *tx)
            .await
            .map_err(storage)?;
        }
        for u in &plan.update {
            // Only what the file filled in changes.
            sqlx::query(
                "UPDATE songs_songs SET category_id = COALESCE(?, category_id), youtube_url = COALESCE(?, youtube_url) \
                 WHERE id = ?",
            )
            .bind(category(&u.category))
            .bind(&u.youtube_url)
            .bind(&u.id)
            .execute(&mut *tx)
            .await
            .map_err(storage)?;
        }
        tx.commit().await.map_err(storage)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    /// A migrated database in memory, with foreign keys on as the Host opens it.
    pub(crate) async fn pool() -> SqlitePool {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        MIGRATOR.run(&pool).await.unwrap();
        pool
    }
}
