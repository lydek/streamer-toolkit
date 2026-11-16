//! Backup files (`.stbackup`, docs/architecture.md §6 資料夾): a zip of `settings.json`, a `VACUUM INTO` snapshot
//! of the database, `templates/` and `sounds/`, plus `manifest.json` with every file's size and SHA-256.
//! Credentials live in the Windows Credential Manager, never in the data folder, so no backup holds them.
//! Admin routes; the caller adds the token check.

use std::{
    io::{self, Cursor, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::{
    admin::{ErrorBody, error},
    db::{self, SqlitePool},
};

const FORMAT: &str = "streamer-toolkit-backup";
const FORMAT_VERSION: u32 = 1;
const MANIFEST: &str = "manifest.json";
const SETTINGS: &str = "settings.json";
/// Data folder subfolders taken whole. Never `logs/`, `cache/`, `WebView2/`, `backups/` or `instance-id`.
const FOLDERS: [&str; 2] = ["templates", "sounds"];
/// What a restore accepts; an export beyond these is refused, so every backup restores.
const MAX_ARCHIVE_BYTES: usize = 512 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;

pub(crate) struct BackupState {
    pub db: SqlitePool,
    pub data_dir: PathBuf,
    pub app_version: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    format: String,
    format_version: u32,
    app_version: String,
    /// The newest migration applied to the database; 0 for an empty one.
    schema_version: i64,
    /// RFC 3339, UTC.
    created_at: String,
    records: Records,
    files: Vec<FileEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Records {
    redemptions: i64,
    checkins: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct FileEntry {
    path: String,
    size: u64,
    sha256: String,
}

#[derive(Debug, thiserror::Error)]
enum BackupError {
    #[error("backup.too_large")]
    TooLarge,
    #[error("backup.export_failed: {0}")]
    Export(String),
}

fn export_failed(e: impl std::fmt::Display) -> BackupError {
    BackupError::Export(e.to_string())
}

pub(crate) fn routes(state: BackupState) -> Router {
    Router::new()
        .route("/api/admin/backup", get(export_backup))
        .with_state(Arc::new(state))
}

/// A `.stbackup` file of this data folder: settings, the database, custom templates and sounds. Live use goes
/// on while it is made; database writes wait only for the snapshot.
#[utoipa::path(
    get,
    path = "/api/admin/backup",
    tag = "admin",
    responses(
        (status = 200, description = "The backup file", content_type = "application/octet-stream", body = String),
        (status = 500, description = "`backup.too_large`: more than a restore accepts; `backup.export_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn export_backup(State(s): State<Arc<BackupState>>) -> Response {
    match export(&s.db, &s.data_dir, &s.app_version).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "application/octet-stream"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            bytes,
        )
            .into_response(),
        Err(e) => {
            tracing::error!(code = "backup.export_failed", error = %e, "backup not made");
            let code = match e {
                BackupError::TooLarge => "backup.too_large",
                BackupError::Export(_) => "backup.export_failed",
            };
            error(StatusCode::INTERNAL_SERVER_ERROR, code)
        }
    }
}

/// The backup file's bytes. The snapshot goes to `cache/` and is removed afterwards.
async fn export(
    pool: &SqlitePool,
    data_dir: &Path,
    app_version: &str,
) -> Result<Vec<u8>, BackupError> {
    let cache = data_dir.join("cache");
    std::fs::create_dir_all(&cache).map_err(export_failed)?;
    let snapshot = cache.join(format!("backup-{}.db", uuid::Uuid::now_v7()));
    let result = export_snapshot(pool, data_dir, app_version, &snapshot).await;
    let _ = std::fs::remove_file(&snapshot);
    result
}

async fn export_snapshot(
    pool: &SqlitePool,
    data_dir: &Path,
    app_version: &str,
    snapshot: &Path,
) -> Result<Vec<u8>, BackupError> {
    db::snapshot(pool, snapshot).await.map_err(export_failed)?;
    let (schema_version, records) = describe(snapshot).await.map_err(export_failed)?;
    let manifest = Manifest {
        format: FORMAT.into(),
        format_version: FORMAT_VERSION,
        app_version: app_version.into(),
        schema_version,
        created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        records,
        files: Vec::new(),
    };
    let (data_dir, snapshot) = (data_dir.to_owned(), snapshot.to_owned());
    tokio::task::spawn_blocking(move || write_archive(&data_dir, &snapshot, manifest))
        .await
        .map_err(export_failed)?
}

/// The snapshot's schema version and record counts, read through the modules' public counts.
async fn describe(snapshot: &Path) -> Result<(i64, Records), sqlx::Error> {
    let pool = db::open_file(snapshot).await?;
    let result = async {
        let version = db::schema_version(&pool).await?;
        // An empty database has no module tables yet.
        let records = if version == 0 {
            Records {
                redemptions: 0,
                checkins: 0,
            }
        } else {
            Records {
                redemptions: redemption::record_count(&pool).await?,
                checkins: checkin::record_count(&pool).await?,
            }
        };
        Ok((version, records))
    }
    .await;
    pool.close().await;
    result
}

fn write_archive(
    data_dir: &Path,
    snapshot: &Path,
    mut manifest: Manifest,
) -> Result<Vec<u8>, BackupError> {
    let mut files = Vec::new();
    let settings = data_dir.join(SETTINGS);
    if settings.is_file() {
        files.push((SETTINGS.to_owned(), settings));
    }
    files.push((db::DB_FILE.to_owned(), snapshot.to_owned()));
    for folder in FOLDERS {
        collect(&data_dir.join(folder), folder, &mut files).map_err(export_failed)?;
    }
    if files.len() >= MAX_ENTRIES {
        return Err(BackupError::TooLarge);
    }
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let mut expanded = 0;
    for (name, path) in files {
        let bytes = std::fs::read(&path).map_err(export_failed)?;
        let size = bytes.len() as u64;
        expanded += size;
        if expanded > MAX_EXPANDED_BYTES {
            return Err(BackupError::TooLarge);
        }
        zip.start_file(name.as_str(), options)
            .map_err(export_failed)?;
        zip.write_all(&bytes).map_err(export_failed)?;
        manifest.files.push(FileEntry {
            path: name,
            size,
            sha256: sha256(&bytes),
        });
    }
    zip.start_file(MANIFEST, options).map_err(export_failed)?;
    let text = serde_json::to_vec_pretty(&manifest).map_err(export_failed)?;
    zip.write_all(&text).map_err(export_failed)?;
    let bytes = zip.finish().map_err(export_failed)?.into_inner();
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(BackupError::TooLarge);
    }
    Ok(bytes)
}

/// Every regular file under `dir`, named `<prefix>/<relative path>`. Symbolic links and junctions are skipped: they
/// may point outside the data folder.
fn collect(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) -> io::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        // Does not follow links.
        let kind = entry.file_type()?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|n| io::Error::other(format!("file name is not Unicode: {n:?}")))?;
        let path = format!("{prefix}/{name}");
        if kind.is_dir() {
            collect(&entry.path(), &path, out)?;
        } else if kind.is_file() {
            // A name a restore would refuse would make a backup that never restores.
            if !allowed_path(&path) {
                return Err(io::Error::other(format!("unsupported file name: {path}")));
            }
            out.push((path, entry.path()));
        }
    }
    Ok(())
}

/// `settings.json`, `toolkit.db`, or a file under `templates/` or `sounds/`, as plain relative segments that are
/// safe to create on Windows.
fn allowed_path(path: &str) -> bool {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let segments: Vec<&str> = path.split('/').collect();
    let plain = |s: &&str| {
        let stem = s.split('.').next().unwrap_or_default();
        !s.is_empty()
            && s.len() <= 255
            && *s != "."
            && *s != ".."
            && !s.ends_with(['.', ' '])
            && !s.contains(['\\', ':', '*', '?', '"', '<', '>', '|'])
            && !s.chars().any(char::is_control)
            && !RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r))
    };
    path.len() <= 1024
        && segments.iter().all(plain)
        && match segments.as_slice() {
            [single] => *single == SETTINGS || *single == db::DB_FILE,
            [folder, _, ..] => FOLDERS.contains(folder),
            [] => false,
        }
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    pub(super) fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-backup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A data folder with something in every place a backup takes or must leave out.
    pub(super) async fn sample_folder(name: &str) -> (PathBuf, SqlitePool) {
        let dir = temp_dir(name);
        toolkit_core::settings::update(&dir, |s| {
            s.twitch_client_id = "clientid123".into();
            s.chat_template = "custom".into();
        })
        .unwrap();
        let write = |path: &str, text: &str| {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write("templates/chat/template.html", "<div data-chat-list></div>");
        write(
            "sounds/0190a1b2-0000-7000-8000-000000000001.mp3",
            "ID3 sound",
        );
        write("logs/streamer-toolkit.2026-11-17.log", "log line");
        write("cache/thing", "cached");
        write("WebView2/Default/Cookies", "cookies");
        write("instance-id", "0123456789abcdef0123456789abcdef");
        write("backups/toolkit-old.db", "old backup");
        let pool = db::open(&dir).await.unwrap();
        for (id, name) in [("r1", "觀眾一"), ("r2", "viewer2")] {
            sqlx::query(
                "INSERT INTO redemption_records (id, twitch_redemption_id, user_id, user_login, user_name, \
                 reward_id, reward_title, reward_cost, user_input, status, redeemed_at, updated_at) \
                 VALUES (?, ?, 'u', 'login', ?, 'reward', 'title', 100, '', 'unfulfilled', \
                 '2026-11-17T00:00:00Z', '2026-11-17T00:00:00Z')",
            )
            .bind(id)
            .bind(format!("twitch-{id}"))
            .bind(name)
            .execute(&pool)
            .await
            .unwrap();
        }
        (dir, pool)
    }

    fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        (0..zip.len())
            .map(|i| {
                let mut file = zip.by_index(i).unwrap();
                let mut content = Vec::new();
                file.read_to_end(&mut content).unwrap();
                (file.name().unwrap().into_owned(), content)
            })
            .collect()
    }

    #[tokio::test]
    async fn export_takes_settings_database_templates_and_sounds_only() {
        let (dir, pool) = sample_folder("export").await;
        let bytes = export(&pool, &dir, "1.2.3").await.unwrap();
        let entries = entries(&bytes);
        let mut names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        names.sort();
        assert_eq!(
            names,
            [
                MANIFEST,
                SETTINGS,
                "sounds/0190a1b2-0000-7000-8000-000000000001.mp3",
                "templates/chat/template.html",
                db::DB_FILE,
            ]
        );

        let (_, text) = entries.iter().find(|(n, _)| n == MANIFEST).unwrap();
        let manifest: Manifest = serde_json::from_slice(text).unwrap();
        assert_eq!(
            (manifest.format.as_str(), manifest.format_version),
            (FORMAT, 1)
        );
        assert_eq!(manifest.app_version, "1.2.3");
        assert!(manifest.schema_version > 0);
        assert_eq!(
            (manifest.records.redemptions, manifest.records.checkins),
            (2, 0)
        );
        assert_eq!(manifest.files.len(), 4);
        for file in &manifest.files {
            let (_, content) = entries.iter().find(|(n, _)| *n == file.path).unwrap();
            assert_eq!(file.size, content.len() as u64);
            assert_eq!(file.sha256, sha256(content));
        }
        // The snapshot is a database of its own and leaves nothing behind in cache/.
        let cache: Vec<_> = std::fs::read_dir(dir.join("cache")).unwrap().collect();
        assert_eq!(cache.len(), 1, "only the sample file");
        pool.close().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn export_holds_no_credentials() {
        let (dir, pool) = sample_folder("secrets").await;
        // What the Credential Manager holds for this folder (docs/architecture.md): never in the data folder.
        let secrets = [
            "access_token",
            "refresh_token",
            "0123456789abcdef0123456789abcdef",
        ];
        let bytes = export(&pool, &dir, "1.2.3").await.unwrap();
        for (name, content) in entries(&bytes) {
            let text = String::from_utf8_lossy(&content).to_lowercase();
            for secret in secrets {
                assert!(!text.contains(secret), "{name} contains {secret}");
            }
            assert!(!text.contains("password"), "{name} mentions a password");
        }
        pool.close().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn only_plain_paths_in_known_places_are_allowed() {
        for ok in [
            "settings.json",
            "toolkit.db",
            "templates/chat/style.css",
            "sounds/a.mp3",
            "templates/chat/圖片/貓.png",
        ] {
            assert!(allowed_path(ok), "{ok}");
        }
        for bad in [
            "",
            "/settings.json",
            "C:/Windows/x",
            "../settings.json",
            "templates/../../x",
            "templates/./x",
            "templates//x",
            "templates",
            "sounds/",
            "templates\\x",
            "logs/a.log",
            "instance-id",
            "settings.json/x",
            "templates/CON",
            "templates/nul.txt",
            "templates/x.",
            "templates/x ",
            "templates/a:b",
            "templates/a\u{0}b",
        ] {
            assert!(!allowed_path(bad), "{bad:?}");
        }
    }
}
