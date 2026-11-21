//! Backup files (`.stbackup`, docs/architecture.md §6 資料夾): a zip of `settings.json`, a `VACUUM INTO` snapshot
//! of the database, `templates/`, `sounds/`, `music/`, `media/` and `checkin/`, plus `manifest.json` with every
//! file's size and SHA-256.
//! Credentials live in the Windows Credential Manager, never in the data folder, so no backup holds them.
//! A restore uploads the file in parts (`upload`), is checked and staged by the admin API, then applied on the next
//! start before the database opens. Admin routes; the caller adds the token check.

use std::{
    collections::HashSet,
    fs,
    io::{self, BufReader, Read, Seek, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    admin::{ErrorBody, error},
    db::{self, SqlitePool},
};

const FORMAT: &str = "streamer-toolkit-backup";
const FORMAT_VERSION: u32 = 1;
const MANIFEST: &str = "manifest.json";
const SETTINGS: &str = "settings.json";
/// Data folder subfolders taken whole. Never `logs/`, `cache/`, `WebView2/`, `backups/` or `instance-id`.
const FOLDERS: [&str; 5] = ["templates", "sounds", "music", "media", "checkin"];
/// What a restore accepts (unpacked size and files); an export beyond these is refused, so every backup restores.
const MAX_EXPANDED_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
/// The largest backup file: the unpacked cap plus room for zip headers and data that does not compress.
const MAX_ARCHIVE_BYTES: u64 = MAX_EXPANDED_BYTES + 1024 * 1024 * 1024;
/// The longest path a backup may hold, in bytes.
const MAX_PATH_BYTES: usize = 1024;
/// A manifest listing `MAX_ENTRIES` files of the longest path, each with its size and SHA-256, pretty-printed.
const MAX_MANIFEST_BYTES: u64 = (MAX_ENTRIES * (MAX_PATH_BYTES + 256) + 64 * 1024) as u64;
/// A checked backup waiting for the restart, in the data folder; its `manifest.json` is written last, once complete.
const STAGING_DIR: &str = "restore-staging";
/// Written when the broadcaster confirms, holding a new restore ID; the next start applies the staged backup.
const PENDING: &str = "restore-pending";
/// The data a restore replaced, kept until the restore has finished so a failure can put it back.
const PREVIOUS_DIR: &str = "restore-previous";
/// In `restore-previous/` before anything moves: the ID of the restore it belongs to. Only that restore's
/// marker puts it back; one left over from an earlier restore never does.
const RESTORE_ID: &str = ".restore-id";
/// In `restore-previous/` once every current item is in it; from then on, items without a copy there came from
/// the backup.
const MOVED: &str = ".moved";
/// What a restore replaces. The database's WAL files go too: SQLite would apply a stale WAL to the restored file.
const REPLACED: [&str; 9] = [
    SETTINGS,
    db::DB_FILE,
    "toolkit.db-wal",
    "toolkit.db-shm",
    "templates",
    "sounds",
    "music",
    "media",
    "checkin",
];
/// What a backup puts in place.
const RESTORED: [&str; 7] = [
    SETTINGS,
    db::DB_FILE,
    "templates",
    "sounds",
    "music",
    "media",
    "checkin",
];

pub(crate) struct BackupState {
    pub db: SqlitePool,
    pub data_dir: PathBuf,
    pub app_version: String,
    /// Restarts the app so the next start applies a confirmed restore.
    pub restart: Arc<dyn Fn() + Send + Sync>,
    /// One staging or confirmation at a time.
    pub lock: tokio::sync::Mutex<()>,
    /// The restore upload in progress; one at a time.
    pub upload: tokio::sync::Mutex<Option<upload::Upload>>,
    /// Free bytes on the disk holding a folder ([`free_space`]; tests pass their own).
    pub free_space: fn(&Path) -> Option<u64>,
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

/// What a backup holds, shown before the broadcaster confirms a restore.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub(crate) struct RestorePreview {
    /// When the backup was made (RFC 3339, UTC).
    pub created_at: String,
    /// The version of the app that made it.
    pub app_version: String,
    pub redemptions: i64,
    pub checkins: i64,
    /// Template folders, e.g. `templates/chat/`.
    pub templates: usize,
    pub sounds: usize,
    pub music: usize,
    pub media: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("backup.too_large")]
    TooLarge,
    /// The disk has less free space than this many bytes, which the export or restore needs.
    #[error("backup.no_space: {0} bytes needed")]
    NoSpace(u64),
    #[error("backup.export_failed: {0}")]
    Export(String),
    /// Damaged, tampered with, or not a backup at all.
    #[error("backup.invalid")]
    Invalid,
    /// Made by a newer version of the app.
    #[error("backup.newer_version")]
    NewerVersion,
    #[error("backup.stage_failed: {0}")]
    Stage(String),
    #[error("backup.nothing_staged")]
    NothingStaged,
    /// The restore failed; the data from before it is back in place.
    #[error("backup.restore_failed: {0}")]
    Restore(String),
    /// A restore was cut short (the app ended mid-way); the data from before it is back in place.
    #[error("backup.restore_interrupted")]
    Interrupted,
    /// The restore failed and the data from before it could not all be put back: it stays in `restore-previous/`
    /// and the next start tries again.
    #[error("backup.put_back_failed: {0}")]
    PutBack(String),
    /// `restore-pending` could not be removed after the restore ended (applied or not). It no longer points at
    /// any `restore-previous/`, so the next start never puts data back because of it.
    #[error("backup.marker_left: {0}")]
    MarkerLeft(String),
}

impl BackupError {
    fn response(&self) -> Response {
        if let Self::NoSpace(needed) = self {
            return (
                StatusCode::INSUFFICIENT_STORAGE,
                Json(serde_json::json!({ "code": "backup.no_space", "needed": needed })),
            )
                .into_response();
        }
        let (status, code) = match self {
            Self::NoSpace(_) => unreachable!("answered above"),
            Self::TooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "backup.too_large"),
            Self::Invalid => (StatusCode::BAD_REQUEST, "backup.invalid"),
            Self::NewerVersion => (StatusCode::BAD_REQUEST, "backup.newer_version"),
            Self::NothingStaged => (StatusCode::CONFLICT, "backup.nothing_staged"),
            Self::Export(_) => (StatusCode::INTERNAL_SERVER_ERROR, "backup.export_failed"),
            Self::Stage(_)
            | Self::Restore(_)
            | Self::Interrupted
            | Self::PutBack(_)
            | Self::MarkerLeft(_) => (StatusCode::INTERNAL_SERVER_ERROR, "backup.restore_failed"),
        };
        if status.is_server_error() {
            tracing::error!(code, error = %self, "backup or restore failed");
        }
        error(status, code)
    }
}

fn export_failed(e: impl std::fmt::Display) -> BackupError {
    BackupError::Export(e.to_string())
}

fn stage_failed(e: impl std::fmt::Display) -> BackupError {
    BackupError::Stage(e.to_string())
}

fn restore_failed(e: impl std::fmt::Display) -> BackupError {
    BackupError::Restore(e.to_string())
}

pub(crate) fn routes(state: BackupState) -> Router {
    Router::new()
        .route("/api/admin/backup", get(export_backup))
        .route("/api/admin/backup/restore/upload", post(upload::start))
        .route(
            "/api/admin/backup/restore/upload/{id}",
            put(upload::part)
                .layer(DefaultBodyLimit::max(upload::PART_MAX_BYTES))
                .delete(upload::cancel),
        )
        .route(
            "/api/admin/backup/restore/upload/{id}/check",
            post(upload::check),
        )
        .route("/api/admin/backup/restore", post(confirm_restore))
        .with_state(Arc::new(state))
}

/// Free bytes on the disk holding `dir`; `None` when unknown.
#[cfg(windows)]
pub(crate) fn free_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    // SAFETY: `wide` is NUL-terminated and outlives the call; the two other outputs are optional.
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free)
}

#[cfg(not(windows))]
pub(crate) fn free_space(_dir: &Path) -> Option<u64> {
    None
}

/// Refuses before anything is written when the disk has less than `needed` free. An unknown amount is not refused:
/// a write that then runs out still fails cleanly.
fn room(needed: u64, free: Option<u64>) -> Result<(), BackupError> {
    match free {
        Some(free) if free < needed => Err(BackupError::NoSpace(needed)),
        _ => Ok(()),
    }
}

/// A `.stbackup` file of this data folder: settings, the database, custom templates, sounds, music, media and card images. Live use goes
/// on while it is made; database writes wait only for the snapshot.
#[utoipa::path(
    get,
    path = "/api/admin/backup",
    tag = "admin",
    responses(
        (status = 200, description = "The backup file", content_type = "application/octet-stream", body = String),
        (status = 413, description = "`backup.too_large`: more than a restore accepts (20 GB unpacked, 100,000 files)", body = ErrorBody),
        (status = 500, description = "`backup.export_failed`", body = ErrorBody),
        (status = 507, description = "`backup.no_space`, with `needed` (bytes): not enough free disk space", body = ErrorBody),
    )
)]
pub(crate) async fn export_backup(State(s): State<Arc<BackupState>>) -> Response {
    match export_file(&s.db, &s.data_dir, &s.app_version).await {
        // The file's name goes now; the open handle streams it and the file is gone once closed.
        Ok(file) => (
            [
                (header::CONTENT_TYPE, "application/octet-stream"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(
                tokio::fs::File::from_std(file.into_file()),
            )),
        )
            .into_response(),
        Err(e) => e.response(),
    }
}

/// Restores the staged backup: the app restarts, backs up the current data to `backups/`, then puts the backup
/// in place before the database opens.
#[utoipa::path(
    post,
    path = "/api/admin/backup/restore",
    tag = "admin",
    responses(
        (status = 202, description = "The app restarts to restore"),
        (status = 409, description = "`backup.nothing_staged`", body = ErrorBody),
        (status = 500, description = "`backup.restore_failed`", body = ErrorBody),
    )
)]
pub(crate) async fn confirm_restore(State(s): State<Arc<BackupState>>) -> Response {
    let _one_at_a_time = s.lock.lock().await;
    if !s.data_dir.join(STAGING_DIR).join(MANIFEST).is_file() {
        return BackupError::NothingStaged.response();
    }
    let id = uuid::Uuid::now_v7().to_string();
    if let Err(e) = fs::write(s.data_dir.join(PENDING), id) {
        return restore_failed(e).response();
    }
    let restart = s.restart.clone();
    tokio::spawn(async move {
        // Lets the response reach the admin window first.
        tokio::time::sleep(Duration::from_millis(500)).await;
        restart();
    });
    StatusCode::ACCEPTED.into_response()
}

/// The backup file, in `cache/` until it is dropped or persisted. The snapshot goes to `cache/` too and is removed
/// afterwards.
async fn export_file(
    pool: &SqlitePool,
    data_dir: &Path,
    app_version: &str,
) -> Result<tempfile::NamedTempFile, BackupError> {
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
) -> Result<tempfile::NamedTempFile, BackupError> {
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
) -> Result<tempfile::NamedTempFile, BackupError> {
    let mut files = Vec::new();
    let settings = data_dir.join(SETTINGS);
    if settings.is_file() {
        files.push((SETTINGS.to_owned(), settings));
    }
    files.push((db::DB_FILE.to_owned(), snapshot.to_owned()));
    for folder in FOLDERS {
        collect(&data_dir.join(folder), folder, &mut files)?;
    }
    if files.len() >= MAX_ENTRIES {
        return Err(BackupError::TooLarge);
    }
    let mut total = 0u64;
    for (_, path) in &files {
        total = total.saturating_add(fs::metadata(path).map_err(export_failed)?.len());
    }
    if total > MAX_EXPANDED_BYTES {
        return Err(BackupError::TooLarge);
    }
    let cache = data_dir.join("cache");
    // The backup file is at most about as large as what it holds.
    room(total, free_space(&cache))?;
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut file = tempfile::NamedTempFile::new_in(&cache).map_err(export_failed)?;
    let mut output = LimitedWriter {
        file: file.as_file_mut(),
        remaining: MAX_ARCHIVE_BYTES,
        exceeded: false,
    };
    let result = (|| {
        let mut zip = ZipWriter::new_stream(&mut output);
        let mut chunk = [0; 32768];
        let mut expanded = 0;
        for (name, path) in files {
            let mut source = fs::File::open(path).map_err(export_failed)?;
            let len = source.metadata().map_err(export_failed)?.len();
            if len > MAX_EXPANDED_BYTES - expanded {
                return Err(BackupError::TooLarge);
            }
            // Zip64 for a file of 4 GiB or more; one that grows past that while it is read fails the export.
            zip.start_file(
                name.as_str(),
                options.large_file(len >= u64::from(u32::MAX)),
            )
            .map_err(export_failed)?;
            let mut size = 0;
            let mut hash = Sha256::new();
            loop {
                let n = source.read(&mut chunk).map_err(export_failed)?;
                if n == 0 {
                    break;
                }
                expanded += n as u64;
                if expanded > MAX_EXPANDED_BYTES {
                    return Err(BackupError::TooLarge);
                }
                size += n as u64;
                hash.update(&chunk[..n]);
                zip.write_all(&chunk[..n]).map_err(export_failed)?;
            }
            manifest.files.push(FileEntry {
                path: name,
                size,
                sha256: hex(&hash.finalize()),
            });
        }
        zip.start_file(MANIFEST, options).map_err(export_failed)?;
        let text = serde_json::to_vec_pretty(&manifest).map_err(export_failed)?;
        if text.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(BackupError::TooLarge);
        }
        zip.write_all(&text).map_err(export_failed)?;
        zip.finish().map_err(export_failed)?;
        Ok(())
    })();
    if output.exceeded {
        return Err(BackupError::TooLarge);
    }
    result?;
    file.rewind().map_err(export_failed)?;
    Ok(file)
}

struct LimitedWriter<'a> {
    file: &'a mut fs::File,
    remaining: u64,
    exceeded: bool,
}

impl Write for LimitedWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            self.exceeded = true;
            return Err(io::Error::other("backup size limit reached"));
        }
        let written = self.file.write(bytes)?;
        self.remaining -= written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
async fn export(
    pool: &SqlitePool,
    data_dir: &Path,
    app_version: &str,
) -> Result<Vec<u8>, BackupError> {
    let mut bytes = Vec::new();
    export_file(pool, data_dir, app_version)
        .await?
        .read_to_end(&mut bytes)
        .map_err(export_failed)?;
    Ok(bytes)
}

/// Every regular file under `dir`, named `<prefix>/<relative path>`. Symbolic links and junctions are skipped: they
/// may point outside the data folder.
fn collect(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), BackupError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(export_failed(e)),
    };
    for entry in entries {
        let entry = entry.map_err(export_failed)?;
        // Does not follow links.
        let kind = entry.file_type().map_err(export_failed)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|n| export_failed(format!("file name is not Unicode: {n:?}")))?;
        let path = format!("{prefix}/{name}");
        if kind.is_dir() {
            collect(&entry.path(), &path, out)?;
        } else if kind.is_file() {
            // A name a restore would refuse would make a backup that never restores.
            if !allowed_path(&path) {
                return Err(export_failed(format!("unsupported file name: {path}")));
            }
            if out.len() >= MAX_ENTRIES - 1 {
                return Err(BackupError::TooLarge);
            }
            out.push((path, entry.path()));
        }
    }
    Ok(())
}

/// `settings.json`, `toolkit.db`, or a file under `templates/`, `sounds/`, `music/`, `media/` or `checkin/`, as plain
/// relative segments that are safe to create on Windows.
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
    path.len() <= MAX_PATH_BYTES
        && segments.iter().all(plain)
        && match segments.as_slice() {
            [single] => *single == SETTINGS || *single == db::DB_FILE,
            [folder, _, ..] => FOLDERS.contains(folder),
            [] => false,
        }
}

#[cfg(test)]
fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Reads `entry` through SHA-256 into `out`, at most one byte more than the manifest says; the size and hash must
/// match it. The zip's own size fields are never trusted.
fn copy_checked(
    entry: impl Read,
    file: &FileEntry,
    out: &mut impl Write,
) -> Result<(), BackupError> {
    let mut entry = entry.take(file.size + 1);
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 64 * 1024];
    let mut size = 0;
    loop {
        // Damaged data (deflate or CRC) fails here.
        let n = entry.read(&mut buf).map_err(|_| BackupError::Invalid)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(stage_failed)?;
    }
    if size != file.size || hex(&hasher.finalize()) != file.sha256 {
        return Err(BackupError::Invalid);
    }
    Ok(())
}

type Archive = ZipArchive<BufReader<fs::File>>;

/// Checks a backup file before anything is written: the limits, every path, each file's size and hash, and that this
/// version can read it. Entries are read in small pieces, so memory does not grow with the backup's size.
fn validate(path: &Path) -> Result<(Manifest, Archive), BackupError> {
    use BackupError::{Invalid, NewerVersion, TooLarge};
    let file = fs::File::open(path).map_err(stage_failed)?;
    if file.metadata().map_err(stage_failed)?.len() > MAX_ARCHIVE_BYTES {
        return Err(TooLarge);
    }
    let mut zip = ZipArchive::new(BufReader::new(file)).map_err(|_| Invalid)?;
    if zip.len() > MAX_ENTRIES {
        return Err(TooLarge);
    }
    let mut text = Vec::new();
    zip.by_name(MANIFEST)
        .map_err(|_| Invalid)?
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut text)
        .map_err(|_| Invalid)?;
    if text.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(Invalid);
    }
    let manifest: Manifest = serde_json::from_slice(&text).map_err(|_| Invalid)?;
    if manifest.format != FORMAT || manifest.format_version == 0 {
        return Err(Invalid);
    }
    if manifest.format_version > FORMAT_VERSION || manifest.schema_version > db::latest_version() {
        return Err(NewerVersion);
    }
    let mut paths = HashSet::new();
    let mut expanded = 0u64;
    for file in &manifest.files {
        if !allowed_path(&file.path) || !paths.insert(file.path.as_str()) {
            return Err(Invalid);
        }
        expanded = expanded.saturating_add(file.size);
    }
    if expanded > MAX_EXPANDED_BYTES {
        return Err(TooLarge);
    }
    // Exactly the listed files plus the manifest: nothing unlisted, nothing twice.
    if !paths.contains(db::DB_FILE) || zip.len() != manifest.files.len() + 1 {
        return Err(Invalid);
    }
    for file in &manifest.files {
        let entry = zip.by_name(&file.path).map_err(|_| Invalid)?;
        copy_checked(entry, file, &mut io::sink())?;
    }
    Ok((manifest, zip))
}

/// Checks the backup file and writes it to `restore-staging/`, replacing anything staged before; on any failure
/// nothing stays staged. The data in use is never touched.
async fn stage(data_dir: &Path, file: &Path) -> Result<RestorePreview, BackupError> {
    let staging = data_dir.join(STAGING_DIR);
    let result = stage_into(&staging, file.to_owned()).await;
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

async fn stage_into(staging: &Path, file: PathBuf) -> Result<RestorePreview, BackupError> {
    let dir = staging.to_owned();
    let manifest = tokio::task::spawn_blocking(move || extract(&file, &dir))
        .await
        .map_err(stage_failed)??;
    // The database itself, not only the manifest's claim: every applied migration must be one this build knows.
    match db::fit(&staging.join(db::DB_FILE))
        .await
        .map_err(|_| BackupError::Invalid)?
    {
        db::Fit::Known => {}
        db::Fit::Newer => return Err(BackupError::NewerVersion),
        db::Fit::Changed => return Err(BackupError::Invalid),
    }
    // Last: only a complete staging has a manifest.
    let text = serde_json::to_vec_pretty(&manifest).map_err(stage_failed)?;
    fs::write(staging.join(MANIFEST), text).map_err(stage_failed)?;
    Ok(preview(&manifest))
}

fn extract(file: &Path, staging: &Path) -> Result<Manifest, BackupError> {
    let (manifest, mut zip) = validate(file)?;
    remove(staging).map_err(stage_failed)?;
    for file in &manifest.files {
        // Checked by `allowed_path`: plain relative segments, so the target stays inside the staging folder.
        let target = staging.join(&file.path);
        fs::create_dir_all(target.parent().unwrap_or(staging)).map_err(stage_failed)?;
        let mut out = fs::File::create(&target).map_err(stage_failed)?;
        let entry = zip.by_name(&file.path).map_err(|_| BackupError::Invalid)?;
        copy_checked(entry, file, &mut out)?;
    }
    // Unreadable settings would silently fall back to defaults after the restart.
    if let Ok(text) = fs::read(staging.join(SETTINGS)) {
        serde_json::from_slice::<toolkit_core::settings::Settings>(&text)
            .map_err(|_| BackupError::Invalid)?;
    }
    Ok(manifest)
}

fn preview(m: &Manifest) -> RestorePreview {
    let under = |folder: &'static str| {
        m.files
            .iter()
            .filter_map(move |f| f.path.strip_prefix(folder)?.strip_prefix('/'))
    };
    RestorePreview {
        created_at: m.created_at.clone(),
        app_version: m.app_version.clone(),
        redemptions: m.records.redemptions,
        checkins: m.records.checkins,
        templates: under("templates")
            .filter_map(|rest| Some(rest.split_once('/')?.0))
            .collect::<HashSet<_>>()
            .len(),
        sounds: under("sounds").count(),
        music: under("music").count(),
        media: under("media").count(),
    }
}

/// Applies a restore confirmed in the admin UI. Call it on start, after the single-instance lock and before the
/// database opens (docs/architecture.md §6). Returns whether a restore was applied. On any failure the data from
/// before is back in place, except for [`BackupError::PutBack`].
pub async fn apply_pending(data_dir: &Path, app_version: &str) -> Result<bool, BackupError> {
    // A restore upload cancelled, cut off or left when the app ended never outlives the run it started in.
    upload::clean(data_dir);
    let pending = data_dir.join(PENDING);
    let staging = data_dir.join(STAGING_DIR);
    let previous = data_dir.join(PREVIOUS_DIR);
    let result = if !pending.exists() {
        Ok(false)
    } else {
        match fs::read_to_string(&pending) {
            Err(e) => Err(restore_failed(e)),
            // The app ended in the middle of this restore.
            Ok(id) if previous.exists() && read_id(&previous) == id => {
                put_back(data_dir, &previous)
                    .map_err(|e| BackupError::PutBack(e.to_string()))
                    .and(Err(BackupError::Interrupted))
            }
            Ok(id) => apply(data_dir, &staging, &previous, &id, app_version)
                .await
                .map(|()| true),
        }
    };
    if matches!(result, Err(BackupError::PutBack(_))) {
        // The marker and the replaced data stay: the next start tries again.
        return result;
    }
    // Unlinked first: if the marker then cannot be removed, the next start neither puts `restore-previous/`
    // back nor applies the staged backup again.
    for link in [previous.join(RESTORE_ID), staging.join(MANIFEST)] {
        if let Err(e) = remove(&link) {
            tracing::warn!(code = "backup.cleanup_failed", file = %link.display(), error = %e, "left behind");
        }
    }
    let marker = remove(&pending);
    for dir in [&previous, &staging] {
        if let Err(e) = remove(dir) {
            tracing::warn!(code = "backup.cleanup_failed", dir = %dir.display(), error = %e, "left behind");
        }
    }
    match (marker, result) {
        (Err(e), Ok(_)) => Err(BackupError::MarkerLeft(e.to_string())),
        (Err(e), Err(r)) => Err(BackupError::MarkerLeft(format!("{r}; {e}"))),
        (Ok(()), result) => result,
    }
}

/// The restore ID in `restore-previous/`; empty when there is none.
fn read_id(previous: &Path) -> String {
    fs::read_to_string(previous.join(RESTORE_ID)).unwrap_or_default()
}

async fn apply(
    data_dir: &Path,
    staging: &Path,
    previous: &Path,
    id: &str,
    app_version: &str,
) -> Result<(), BackupError> {
    if !staging.join(MANIFEST).is_file() {
        return Err(BackupError::NothingStaged);
    }
    // Left by an earlier restore that finished but could not clean up; what it holds was saved to `backups/`
    // before that restore. Never put back.
    remove(previous)
        .map_err(|e| restore_failed(format!("removing an old {PREVIOUS_DIR}/: {e}")))?;
    back_up_current(data_dir, app_version).await?;
    let result = async {
        swap(data_dir, staging, previous, id).map_err(restore_failed)?;
        // The normal start path: pending migrations run, after their own backup.
        db::open(data_dir)
            .await
            .map_err(restore_failed)?
            .close()
            .await;
        Ok(())
    }
    .await;
    if let Err(e) = result {
        put_back(data_dir, previous).map_err(|p| BackupError::PutBack(format!("{e}; {p}")))?;
        return Err(e);
    }
    Ok(())
}

/// Saves the current data as a `.stbackup` in `backups/`, so the admin UI can restore it again.
async fn back_up_current(data_dir: &Path, app_version: &str) -> Result<PathBuf, BackupError> {
    let db_file = data_dir.join(db::DB_FILE);
    if !db_file.exists() {
        // An empty file is an empty database.
        fs::write(&db_file, b"").map_err(restore_failed)?;
    }
    let pool = db::open_file(&db_file).await.map_err(restore_failed)?;
    let archive = export_file(&pool, data_dir, app_version).await;
    pool.close().await;
    let archive = archive.map_err(|e| match e {
        // Same limits as an export; the restore stops before anything moves.
        BackupError::TooLarge | BackupError::NoSpace(_) => e,
        e => restore_failed(format!("backing up the current data: {e}")),
    })?;
    let dir = data_dir.join(db::BACKUP_DIR);
    fs::create_dir_all(&dir).map_err(restore_failed)?;
    let path = dir.join(format!("before-restore-{}.stbackup", db::stamp()));
    // `cache/` and `backups/` are both in the data folder: a rename, not a second copy.
    archive.persist(&path).map_err(restore_failed)?;
    tracing::info!(backup = %path.display(), "current data backed up before restoring");
    Ok(path)
}

/// Moves the current items into `restore-previous/`, then the staged ones into place; renames only, all inside the
/// data folder.
fn swap(data_dir: &Path, staging: &Path, previous: &Path, id: &str) -> io::Result<()> {
    fs::create_dir_all(previous)?;
    fs::write(previous.join(RESTORE_ID), id)?;
    for item in REPLACED {
        let current = data_dir.join(item);
        if fs::symlink_metadata(&current).is_ok() {
            fs::rename(&current, previous.join(item))?;
        }
    }
    fs::write(previous.join(MOVED), b"")?;
    for item in RESTORED {
        let staged = staging.join(item);
        if staged.exists() {
            fs::rename(&staged, data_dir.join(item))?;
        }
    }
    Ok(())
}

/// Undoes [`swap`], also one cut short: what came from the backup goes, the replaced items come back.
fn put_back(data_dir: &Path, previous: &Path) -> io::Result<()> {
    let all_moved = previous.join(MOVED).exists();
    for item in REPLACED {
        let (current, kept) = (data_dir.join(item), previous.join(item));
        let has_kept = fs::symlink_metadata(&kept).is_ok();
        // Before every item was moved, one without a copy is still the original.
        if has_kept || all_moved {
            remove(&current)?;
        }
        if has_kept {
            fs::rename(&kept, &current)?;
        }
    }
    Ok(())
}

/// Removes a file or a folder; a missing one is fine.
fn remove(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

pub(crate) mod upload;

#[cfg(test)]
mod tests;
