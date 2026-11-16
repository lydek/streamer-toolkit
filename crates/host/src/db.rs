//! The single SQLite database (docs/architecture.md §4): every module's migrations merged into one Migrator, with
//! a backup taken before any pending migration runs.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

pub use sqlx::SqlitePool;
use sqlx::{
    migrate::{MigrateError, Migration, Migrator},
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

/// The database file in the data folder.
pub const DB_FILE: &str = "toolkit.db";
/// Pre-migration backups in the data folder; never deleted automatically.
pub const BACKUP_DIR: &str = "backups";

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("db.open_failed: {0}")]
    Open(#[from] sqlx::Error),
    #[error("db.backup_failed: {0}")]
    Backup(String),
    #[error("db.migration_failed: {0}")]
    Migrate(#[from] MigrateError),
    #[error("db.duplicate_migration: {0}")]
    DuplicateVersion(i64),
}

/// Every module's migrations, in one list. Disabled modules are listed too: a missing version fails the start.
fn modules() -> Vec<&'static Migrator> {
    vec![&redemption::MIGRATOR, &checkin::MIGRATOR]
}

/// Merges the modules' migrations in version order; two modules sharing a version is an error.
fn merged(sets: &[&Migrator]) -> Result<Vec<Migration>, DbError> {
    let mut all: Vec<Migration> = sets.iter().flat_map(|m| m.iter().cloned()).collect();
    all.sort_by_key(|m| m.version);
    if let Some(pair) = all.windows(2).find(|w| w[0].version == w[1].version) {
        return Err(DbError::DuplicateVersion(pair[0].version));
    }
    Ok(all)
}

/// Opens `<data_dir>/toolkit.db`, creating it on first start, and applies pending migrations. A database that
/// already has migrations is backed up first; if the backup fails, nothing is applied.
pub async fn open(data_dir: &Path) -> Result<SqlitePool, DbError> {
    open_with(data_dir, merged(&modules())?).await
}

async fn open_with(data_dir: &Path, migrations: Vec<Migration>) -> Result<SqlitePool, DbError> {
    let options = SqliteConnectOptions::new()
        .filename(data_dir.join(DB_FILE))
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);
    // shortcut: one connection is the single writer, so writes never hit SQLITE_BUSY; reads queue behind writes,
    // add a separate read pool if that ever shows up as lag.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let migrator = Migrator::with_migrations(migrations);
    let applied = applied_versions(&pool).await?;
    // A fresh database has nothing to lose; an upgrade with nothing pending needs no copy.
    if !applied.is_empty() && migrator.iter().any(|m| !applied.contains(&m.version)) {
        let path = backup(&pool, data_dir).await?;
        tracing::info!(backup = %path.display(), "database backed up before migrating");
    }
    // A database from a newer version (an applied migration this build does not know) fails here.
    migrator.run(&pool).await?;
    Ok(pool)
}

async fn applied_versions(pool: &SqlitePool) -> Result<HashSet<i64>, sqlx::Error> {
    let exists: Option<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_optional(pool)
    .await?;
    if exists.is_none() {
        return Ok(HashSet::new());
    }
    let rows: Vec<(i64,)> = sqlx::query_as("SELECT version FROM _sqlx_migrations")
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|(v,)| v).collect())
}

/// A consistent copy taken through SQLite (`VACUUM INTO`), never a raw copy of the open file.
async fn backup(pool: &SqlitePool, data_dir: &Path) -> Result<PathBuf, DbError> {
    let failed = |e: &dyn std::fmt::Display| DbError::Backup(e.to_string());
    let dir = data_dir.join(BACKUP_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| failed(&e))?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%3fZ");
    let path = dir.join(format!("toolkit-{stamp}.db"));
    let target = path.to_str().ok_or_else(|| failed(&"path is not UTF-8"))?;
    // VACUUM INTO refuses an existing file, so a backup never overwrites another.
    sqlx::query("VACUUM INTO ?")
        .bind(target)
        .execute(pool)
        .await
        .map_err(|e| failed(&e))?;
    Ok(path)
}

/// An empty in-memory database for tests that never touch module tables.
#[cfg(test)]
pub(crate) fn memory() -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_lazy("sqlite::memory:")
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::{SqlSafeStr, migrate::MigrationType};

    fn migration(version: i64, sql: &'static str) -> Migration {
        Migration::new(
            version,
            "test".into(),
            MigrationType::Simple,
            sql.into_sql_str(),
            false,
        )
    }

    fn create_notes() -> Migration {
        migration(
            20261116000001,
            "CREATE TABLE test_notes (id INTEGER PRIMARY KEY, text TEXT NOT NULL)",
        )
    }

    fn add_column() -> Migration {
        migration(
            20261116000002,
            "ALTER TABLE test_notes ADD COLUMN extra TEXT",
        )
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-db-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn backups(dir: &Path) -> Vec<PathBuf> {
        match std::fs::read_dir(dir.join(BACKUP_DIR)) {
            Ok(entries) => entries.map(|e| e.unwrap().path()).collect(),
            Err(_) => vec![],
        }
    }

    async fn notes(pool: &SqlitePool) -> Vec<String> {
        sqlx::query_as::<_, (String,)>("SELECT text FROM test_notes ORDER BY id")
            .fetch_all(pool)
            .await
            .unwrap()
            .into_iter()
            .map(|(t,)| t)
            .collect()
    }

    async fn connect(path: &Path) -> SqlitePool {
        SqlitePool::connect_with(SqliteConnectOptions::new().filename(path))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn first_start_creates_the_database_without_a_backup() {
        let dir = temp_dir("fresh");
        let pool = open_with(&dir, vec![create_notes()]).await.unwrap();
        assert!(notes(&pool).await.is_empty(), "migration applied");
        pool.close().await;
        assert!(dir.join(DB_FILE).exists());
        assert!(backups(&dir).is_empty());

        // Nothing new to apply: still no backup.
        let pool = open_with(&dir, vec![create_notes()]).await.unwrap();
        pool.close().await;
        assert!(backups(&dir).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn new_migration_backs_up_first_and_keeps_the_data() {
        let dir = temp_dir("upgrade");
        let pool = open_with(&dir, vec![create_notes()]).await.unwrap();
        sqlx::query("INSERT INTO test_notes (text) VALUES ('kept')")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        let pool = open_with(&dir, vec![create_notes(), add_column()])
            .await
            .unwrap();
        assert_eq!(notes(&pool).await, ["kept"], "upgrade keeps the data");
        pool.close().await;

        // The backup restores: it opens as a database with the data from before the migration.
        let [backup] = backups(&dir).try_into().unwrap();
        let restored = connect(&backup).await;
        assert_eq!(notes(&restored).await, ["kept"]);
        let columns: Vec<(String,)> =
            sqlx::query_as("SELECT name FROM pragma_table_info('test_notes')")
                .fetch_all(&restored)
                .await
                .unwrap();
        assert!(
            !columns.iter().any(|(c,)| c == "extra"),
            "taken before the migration"
        );
        restored.close().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn failed_backup_applies_nothing() {
        let dir = temp_dir("backup-fails");
        open_with(&dir, vec![create_notes()])
            .await
            .unwrap()
            .close()
            .await;
        // A file where the backup folder should be.
        std::fs::write(dir.join(BACKUP_DIR), "").unwrap();

        let result = open_with(&dir, vec![create_notes(), add_column()]).await;
        assert!(matches!(result, Err(DbError::Backup(_))), "{result:?}");
        let pool = connect(&dir.join(DB_FILE)).await;
        let applied = applied_versions(&pool).await.unwrap();
        assert_eq!(applied, HashSet::from([create_notes().version]));
        pool.close().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn database_from_a_newer_version_is_refused() {
        let dir = temp_dir("newer");
        open_with(&dir, vec![create_notes(), add_column()])
            .await
            .unwrap()
            .close()
            .await;
        let result = open_with(&dir, vec![create_notes()]).await;
        assert!(
            matches!(
                result,
                Err(DbError::Migrate(MigrateError::VersionMissing(_)))
            ),
            "{result:?}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn merged_versions_are_unique_and_ordered() {
        let a = Migrator::with_migrations(vec![add_column()]);
        let b = Migrator::with_migrations(vec![create_notes()]);
        let versions: Vec<i64> = merged(&[&a, &b])
            .unwrap()
            .iter()
            .map(|m| m.version)
            .collect();
        assert_eq!(versions, [create_notes().version, add_column().version]);

        let clash = Migrator::with_migrations(vec![migration(add_column().version, "SELECT 1")]);
        assert!(matches!(
            merged(&[&a, &clash]),
            Err(DbError::DuplicateVersion(v)) if v == add_column().version
        ));

        // The real list: strictly increasing means unique and ordered.
        let real = merged(&modules()).unwrap();
        assert!(real.windows(2).all(|w| w[0].version < w[1].version));
    }
}
