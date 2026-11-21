use std::{
    io::Cursor,
    sync::atomic::{AtomicBool, Ordering},
};

use axum::{body::Body, http::Request};
use tower::ServiceExt;

use super::*;

/// Stages a backup given as bytes, through a file as an upload does.
async fn stage_bytes(dir: &Path, bytes: Vec<u8>) -> Result<RestorePreview, BackupError> {
    let file = tempfile::NamedTempFile::new().unwrap();
    fs::write(file.path(), bytes).unwrap();
    stage(dir, file.path()).await
}

#[test]
fn output_limit_rejects_writes_before_the_file_grows() {
    let mut file = tempfile::tempfile().unwrap();
    let mut limited = LimitedWriter {
        file: &mut file,
        remaining: 4,
        exceeded: false,
    };
    limited.write_all(b"1234").unwrap();
    assert!(limited.write_all(b"5").is_err());
    assert!(limited.exceeded);
    assert_eq!(file.metadata().unwrap().len(), 4);
}

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
    write(
        "music/0190a1b2-0000-7000-8000-000000000002.ogg",
        "OggS music",
    );
    write(
        "media/0190a1b2-0000-7000-8000-000000000003.gif",
        "GIF89a media",
    );
    write(
        "checkin/0190a1b2-0000-7000-8000-000000000004.png",
        "PNG card",
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
    sqlx::query(
        "INSERT INTO activity_events (id, twitch_message_id, kind, user_name, occurred_at)              VALUES ('a1', 'm1', 'follow', '觀眾一', '2026-11-17T00:00:00.000000Z')",
    )
    .execute(&pool)
    .await
    .unwrap();
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
async fn export_takes_settings_database_templates_sounds_music_media_and_card_images_only() {
    let (dir, pool) = sample_folder("export").await;
    let bytes = export(&pool, &dir, "1.2.3").await.unwrap();
    let entries = entries(&bytes);
    let mut names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    names.sort();
    assert_eq!(
        names,
        [
            "checkin/0190a1b2-0000-7000-8000-000000000004.png",
            MANIFEST,
            "media/0190a1b2-0000-7000-8000-000000000003.gif",
            "music/0190a1b2-0000-7000-8000-000000000002.ogg",
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
    assert_eq!(manifest.files.len(), 7);
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
        "music/a.ogg",
        "media/a.gif",
        "checkin/a.png",
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
        "music/",
        "media/",
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

/// What a restore must bring back: settings, templates, sounds, music and media, and database rows.
#[derive(Debug, PartialEq)]
struct FolderState {
    settings: Vec<u8>,
    files: Vec<(String, Vec<u8>)>,
    rows: Vec<(String, String)>,
    activity: Vec<String>,
}

async fn state_of(dir: &Path) -> FolderState {
    let mut paths = Vec::new();
    for folder in FOLDERS {
        collect(&dir.join(folder), folder, &mut paths).unwrap();
    }
    let mut files: Vec<_> = paths
        .into_iter()
        .map(|(name, path)| (name, fs::read(path).unwrap()))
        .collect();
    files.sort();
    let pool = db::open_file(&dir.join(db::DB_FILE)).await.unwrap();
    let rows = sqlx::query_as("SELECT id, user_name FROM redemption_records ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    let activity = sqlx::query_scalar("SELECT twitch_message_id FROM activity_events ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    pool.close().await;
    FolderState {
        settings: fs::read(dir.join(SETTINGS)).unwrap_or_default(),
        files,
        rows,
        activity,
    }
}

fn leftovers(dir: &Path) -> Vec<&'static str> {
    [STAGING_DIR, PENDING, PREVIOUS_DIR]
        .into_iter()
        .filter(|l| dir.join(l).exists())
        .collect()
}

fn backup_state(
    dir: &Path,
    pool: SqlitePool,
    restarted: Arc<AtomicBool>,
    free_space: fn(&Path) -> Option<u64>,
) -> BackupState {
    BackupState {
        db: pool,
        data_dir: dir.to_owned(),
        app_version: "1.2.3".into(),
        restart: Arc::new(move || restarted.store(true, Ordering::SeqCst)),
        lock: Default::default(),
        upload: Default::default(),
        free_space,
    }
}

fn test_state(dir: &Path, pool: SqlitePool, restarted: Arc<AtomicBool>) -> Arc<BackupState> {
    Arc::new(backup_state(dir, pool, restarted, |_| None))
}

fn repack(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in entries {
        zip.start_file(name.as_str(), SimpleFileOptions::default())
            .unwrap();
        zip.write_all(content).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// The backup with its manifest changed as JSON.
fn with_manifest(bytes: &[u8], edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let mut entries = entries(bytes);
    let (_, text) = entries.iter_mut().find(|(n, _)| n == MANIFEST).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_slice(text).unwrap();
    edit(&mut manifest);
    *text = serde_json::to_vec(&manifest).unwrap();
    repack(&entries)
}

/// The backup with one file renamed and replaced, its manifest entry matching: only other checks can catch it.
fn with_file(bytes: &[u8], path: &str, new_path: &str, content: &[u8]) -> Vec<u8> {
    let mut entries = entries(bytes);
    let entry = entries.iter_mut().find(|(n, _)| n == path).unwrap();
    *entry = (new_path.to_owned(), content.to_vec());
    let renamed = repack(&entries);
    with_manifest(&renamed, |m| {
        let files = m["files"].as_array_mut().unwrap();
        let file = files.iter_mut().find(|f| f["path"] == path).unwrap();
        file["path"] = new_path.into();
        file["size"] = content.len().into();
        file["sha256"] = sha256(content).into();
    })
}

#[tokio::test]
async fn export_then_restore_brings_back_the_same_settings_and_rows() {
    let (dir, pool) = sample_folder("round-trip").await;
    let exported = export(&pool, &dir, "1.2.3").await.unwrap();
    let before = state_of(&dir).await;
    // Everything changes after the export.
    toolkit_core::settings::update(&dir, |s| s.twitch_client_id = "changed".into()).unwrap();
    sqlx::query("DELETE FROM redemption_records WHERE id = 'r1'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM activity_events")
        .execute(&pool)
        .await
        .unwrap();
    fs::remove_dir_all(dir.join("templates")).unwrap();
    fs::write(dir.join("sounds/new.mp3"), "new").unwrap();
    fs::remove_dir_all(dir.join("music")).unwrap();
    fs::remove_dir_all(dir.join("media")).unwrap();
    fs::remove_dir_all(dir.join("checkin")).unwrap();
    let changed = state_of(&dir).await;
    assert_ne!(changed, before);

    let preview = stage_bytes(&dir, exported).await.unwrap();
    assert_eq!(preview.app_version, "1.2.3");
    assert_eq!(
        (
            preview.redemptions,
            preview.checkins,
            preview.templates,
            preview.sounds,
            preview.music,
            preview.media
        ),
        (2, 0, 1, 1, 1, 1)
    );
    assert_eq!(
        state_of(&dir).await,
        changed,
        "staging changes nothing in use"
    );
    let restarted = Arc::new(AtomicBool::new(false));
    let state = test_state(&dir, pool.clone(), restarted.clone());
    assert_eq!(
        confirm_restore(State(state)).await.status(),
        StatusCode::ACCEPTED
    );
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(restarted.load(Ordering::SeqCst), "the app restarts");
    assert_eq!(
        state_of(&dir).await,
        changed,
        "nothing changes before the restart"
    );
    pool.close().await;

    // The next start.
    assert!(apply_pending(&dir, "1.2.4").await.unwrap());
    assert_eq!(state_of(&dir).await, before);
    assert_eq!(
        fs::read_to_string(dir.join("instance-id")).unwrap(),
        "0123456789abcdef0123456789abcdef",
        "credentials stay keyed by this folder"
    );
    assert!(dir.join("logs/streamer-toolkit.2026-11-17.log").exists());
    assert!(leftovers(&dir).is_empty(), "{:?}", leftovers(&dir));
    assert!(!apply_pending(&dir, "1.2.4").await.unwrap(), "applied once");

    // The data from before the restore was backed up to backups/, and restores too.
    let [saved] = fs::read_dir(dir.join(db::BACKUP_DIR))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().contains("before-restore-"))
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    assert!(saved.to_string_lossy().ends_with(".stbackup"));
    stage_bytes(&dir, fs::read(saved).unwrap()).await.unwrap();
    fs::write(dir.join(PENDING), b"").unwrap();
    assert!(apply_pending(&dir, "1.2.4").await.unwrap());
    assert_eq!(state_of(&dir).await, changed);
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn damaged_or_foreign_files_are_refused_and_nothing_changes() {
    let (dir, pool) = sample_folder("refused").await;
    let good = export(&pool, &dir, "1.2.3").await.unwrap();
    let before = state_of(&dir).await;
    let template = "templates/chat/template.html";
    let mut entries = entries(&good);
    let tampered = {
        let mut e = entries.clone();
        e.iter_mut().find(|(n, _)| n == template).unwrap().1 = b"<b>changed</b>".to_vec();
        repack(&e)
    };
    let unlisted = {
        let mut e = entries.clone();
        e.push(("sounds/unlisted.mp3".into(), b"x".to_vec()));
        repack(&e)
    };
    let no_manifest = repack(&entries[..entries.len() - 1]);
    entries.extend((0..MAX_ENTRIES).map(|i| (format!("sounds/{i}.mp3"), Vec::new())));
    let too_many = repack(&entries);
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        ("not a zip", b"not a backup".to_vec(), "backup.invalid"),
        ("tampered", tampered, "backup.invalid"),
        ("unlisted file", unlisted, "backup.invalid"),
        ("no manifest", no_manifest, "backup.invalid"),
        ("too many files", too_many, "backup.too_large"),
        (
            "zip slip",
            with_file(&good, template, "templates/../../evil.html", b"x"),
            "backup.invalid",
        ),
        (
            "absolute path",
            with_file(&good, template, "C:/evil.html", b"x"),
            "backup.invalid",
        ),
        (
            "not a data folder file",
            with_file(&good, template, "instance-id", b"x"),
            "backup.invalid",
        ),
        (
            "unreadable settings",
            with_file(&good, SETTINGS, SETTINGS, br#"{"port":"x"}"#),
            "backup.invalid",
        ),
        (
            "another format",
            with_manifest(&good, |m| m["format"] = "zip".into()),
            "backup.invalid",
        ),
        (
            "expands too far",
            with_manifest(&good, |m| m["files"][0]["size"] = (21u64 << 30).into()),
            "backup.too_large",
        ),
        (
            "newer format",
            with_manifest(&good, |m| m["format_version"] = 3.into()),
            "backup.newer_version",
        ),
        (
            "format 1 with one kind",
            with_manifest(&good, |m| m["contents"] = serde_json::json!(["data"])),
            "backup.invalid",
        ),
        (
            "no kinds",
            with_manifest(&good, |m| {
                m["format_version"] = 2.into();
                m["contents"] = serde_json::json!([]);
            }),
            "backup.invalid",
        ),
        (
            "data only, holding media",
            with_manifest(&good, |m| {
                m["format_version"] = 2.into();
                m["contents"] = serde_json::json!(["data"]);
            }),
            "backup.invalid",
        ),
        (
            "media only, holding the database",
            with_manifest(&good, |m| {
                m["format_version"] = 2.into();
                m["contents"] = serde_json::json!(["media"]);
            }),
            "backup.invalid",
        ),
        (
            "newer schema",
            with_manifest(&good, |m| {
                m["schema_version"] = (db::latest_version() + 1).into()
            }),
            "backup.newer_version",
        ),
    ];
    for (label, bytes, code) in cases {
        let e = stage_bytes(&dir, bytes)
            .await
            .err()
            .unwrap_or_else(|| panic!("{label} accepted"));
        assert_eq!(e.to_string(), code, "{label}");
        assert!(leftovers(&dir).is_empty(), "{label}: {:?}", leftovers(&dir));
        assert_eq!(state_of(&dir).await, before, "{label}");
    }
    // Nothing staged: there is nothing to confirm.
    let state = test_state(&dir, pool.clone(), Arc::default());
    assert_eq!(
        confirm_restore(State(state)).await.status(),
        StatusCode::CONFLICT
    );
    pool.close().await;
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_database_from_a_newer_version_is_refused_even_if_the_manifest_hides_it() {
    let (dir, pool) = sample_folder("newer").await;
    sqlx::query(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (?, 'future', 1, x'00', 0)",
    )
    .bind(db::latest_version() + 1)
    .execute(&pool)
    .await
    .unwrap();
    let newer = export(&pool, &dir, "9.0.0").await.unwrap();
    let before = state_of(&dir).await;
    let e = stage_bytes(&dir, newer.clone()).await.unwrap_err();
    assert_eq!(e.to_string(), "backup.newer_version");
    let hidden = with_manifest(&newer, |m| m["schema_version"] = 1.into());
    let e = stage_bytes(&dir, hidden).await.unwrap_err();
    assert_eq!(e.to_string(), "backup.newer_version");
    assert!(leftovers(&dir).is_empty());
    assert_eq!(state_of(&dir).await, before);
    pool.close().await;
    let _ = fs::remove_dir_all(dir);
}

// An open file keeps Windows from moving its folder, which makes the swap fail half-way.
#[cfg(windows)]
#[tokio::test]
async fn a_restore_that_fails_half_way_puts_the_data_back() {
    let (dir, pool) = sample_folder("half-way").await;
    let bytes = export(&pool, &dir, "1.2.3").await.unwrap();
    sqlx::query("DELETE FROM redemption_records")
        .execute(&pool)
        .await
        .unwrap();
    fs::write(dir.join("templates/chat/template.html"), "changed").unwrap();
    stage_bytes(&dir, bytes).await.unwrap();
    fs::write(dir.join(PENDING), b"").unwrap();
    pool.close().await;
    let before = state_of(&dir).await;

    let held = fs::File::open(dir.join("sounds/0190a1b2-0000-7000-8000-000000000001.mp3")).unwrap();
    let e = apply_pending(&dir, "1.2.3").await.unwrap_err();
    drop(held);
    assert!(matches!(e, BackupError::Restore(_)), "{e}");
    assert_eq!(state_of(&dir).await, before);
    assert!(leftovers(&dir).is_empty(), "{:?}", leftovers(&dir));
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_restore_cut_short_is_undone_on_the_next_start() {
    let dir = temp_dir("interrupted");
    // As a swap leaves the folder when the app ends after moving everything out and part of the backup in.
    let previous = dir.join(PREVIOUS_DIR);
    fs::create_dir_all(previous.join("templates")).unwrap();
    fs::write(previous.join(SETTINGS), "old settings").unwrap();
    fs::write(previous.join(db::DB_FILE), "old db").unwrap();
    fs::write(previous.join("templates/old.css"), "old").unwrap();
    fs::write(previous.join(MOVED), "").unwrap();
    fs::write(dir.join(db::DB_FILE), "restored db").unwrap();
    fs::create_dir_all(dir.join("sounds")).unwrap();
    fs::write(dir.join("sounds/restored.mp3"), "restored").unwrap();
    fs::create_dir_all(dir.join(STAGING_DIR)).unwrap();
    fs::write(dir.join(PENDING), "").unwrap();

    let e = apply_pending(&dir, "1.2.3").await.unwrap_err();
    assert!(matches!(e, BackupError::Interrupted), "{e}");
    assert_eq!(
        fs::read_to_string(dir.join(SETTINGS)).unwrap(),
        "old settings"
    );
    assert_eq!(fs::read_to_string(dir.join(db::DB_FILE)).unwrap(), "old db");
    assert!(dir.join("templates/old.css").exists());
    assert!(!dir.join("sounds").exists(), "came from the backup");
    assert!(leftovers(&dir).is_empty(), "{:?}", leftovers(&dir));
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_restore_previous_left_by_an_earlier_restore_is_never_put_back() {
    let (dir, pool) = sample_folder("stale-previous").await;
    let exported = export(&pool, &dir, "1.2.3").await.unwrap();
    let before = state_of(&dir).await;
    toolkit_core::settings::update(&dir, |s| s.twitch_client_id = "changed".into()).unwrap();
    let changed = state_of(&dir).await;
    // An earlier restore finished but could not remove its folder.
    let previous = dir.join(PREVIOUS_DIR);
    fs::create_dir_all(&previous).unwrap();
    fs::write(previous.join(RESTORE_ID), "earlier").unwrap();
    fs::write(previous.join(MOVED), "").unwrap();
    fs::write(previous.join(SETTINGS), "stale settings").unwrap();
    stage_bytes(&dir, exported).await.unwrap();
    let state = test_state(&dir, pool.clone(), Arc::default());
    assert_eq!(
        confirm_restore(State(state)).await.status(),
        StatusCode::ACCEPTED
    );
    pool.close().await;

    assert!(apply_pending(&dir, "1.2.3").await.unwrap());
    assert_eq!(
        state_of(&dir).await,
        before,
        "the confirmed backup, never the leftover"
    );
    assert!(leftovers(&dir).is_empty(), "{:?}", leftovers(&dir));
    // The data in use until then was backed up first.
    let [saved] = fs::read_dir(dir.join(db::BACKUP_DIR))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().contains("before-restore-"))
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    stage_bytes(&dir, fs::read(saved).unwrap()).await.unwrap();
    fs::write(dir.join(PENDING), "again").unwrap();
    assert!(apply_pending(&dir, "1.2.3").await.unwrap());
    assert_eq!(state_of(&dir).await, changed);
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_restored_database_whose_migration_fails_is_put_back_in_full() {
    let (dir, pool) = sample_folder("failing-migration").await;
    pool.close().await;
    let before = state_of(&dir).await;
    let staging = dir.join(STAGING_DIR);
    fs::create_dir_all(&staging).unwrap();
    let staged = SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(staging.join(db::DB_FILE))
            .create_if_missing(true),
    )
    .await
    .unwrap();
    redemption::MIGRATOR.run(&staged).await.unwrap();
    // A later migration creates this table, so it fails.
    sqlx::query("CREATE TABLE checkin_viewers (id TEXT)")
        .execute(&staged)
        .await
        .unwrap();
    staged.close().await;
    fs::write(staging.join(MANIFEST), "{}").unwrap();
    fs::write(dir.join(PENDING), "r1").unwrap();

    let e = apply_pending(&dir, "1.2.3").await.unwrap_err();
    assert!(matches!(e, BackupError::Restore(_)), "{e}");
    assert_eq!(state_of(&dir).await, before);
    assert!(leftovers(&dir).is_empty(), "{:?}", leftovers(&dir));
    let _ = fs::remove_dir_all(dir);
}

// A file held without delete sharing cannot be removed on Windows.
#[cfg(windows)]
#[tokio::test]
async fn a_marker_that_cannot_be_removed_is_reported_and_never_undoes_the_restore() {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 1;
    let (dir, pool) = sample_folder("marker-left").await;
    let exported = export(&pool, &dir, "1.2.3").await.unwrap();
    let before = state_of(&dir).await;
    toolkit_core::settings::update(&dir, |s| s.twitch_client_id = "changed".into()).unwrap();
    stage_bytes(&dir, exported).await.unwrap();
    fs::write(dir.join(PENDING), "r1").unwrap();
    pool.close().await;

    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(dir.join(PENDING))
        .unwrap();
    let e = apply_pending(&dir, "1.2.3").await.unwrap_err();
    drop(held);
    assert!(matches!(e, BackupError::MarkerLeft(_)), "{e}");
    assert_eq!(state_of(&dir).await, before, "the restore went through");
    // The next start puts nothing back and applies nothing again.
    let e = apply_pending(&dir, "1.2.3").await.unwrap_err();
    assert!(matches!(e, BackupError::NothingStaged), "{e}");
    assert_eq!(state_of(&dir).await, before);
    assert!(leftovers(&dir).is_empty(), "{:?}", leftovers(&dir));
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_staged_backup_never_confirmed_is_dropped_on_start() {
    let (dir, pool) = sample_folder("unconfirmed").await;
    let bytes = export(&pool, &dir, "1.2.3").await.unwrap();
    stage_bytes(&dir, bytes).await.unwrap();
    pool.close().await;
    let before = state_of(&dir).await;
    assert!(!apply_pending(&dir, "1.2.3").await.unwrap());
    assert_eq!(state_of(&dir).await, before);
    assert!(leftovers(&dir).is_empty());
    let _ = fs::remove_dir_all(dir);
}

/// Sends one request to the backup routes; returns the status and the JSON body (`null` when empty).
async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    body: Body,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(body)
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

fn uploads(dir: &Path) -> usize {
    fs::read_dir(dir.join("cache"))
        .map(|d| {
            d.flatten()
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with("restore-upload-")
                })
                .count()
        })
        .unwrap_or(0)
}

#[tokio::test]
async fn a_backup_uploaded_in_parts_is_checked_from_the_file_and_leaves_no_upload_behind() {
    let (dir, pool) = sample_folder("upload").await;
    let bytes = export(&pool, &dir, "1.2.3").await.unwrap();
    let before = state_of(&dir).await;
    let app = routes(backup_state(&dir, pool.clone(), Arc::default(), |_| {
        Some(u64::MAX)
    }));
    let size = bytes.len();
    let (status, started) = send(
        &app,
        "POST",
        "/api/admin/backup/restore/upload",
        Body::from(format!(r#"{{"size":{size}}}"#)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = started["id"].as_str().unwrap().to_owned();
    let part_uri = |offset: usize| format!("/api/admin/backup/restore/upload/{id}?offset={offset}");
    let check_uri = format!("/api/admin/backup/restore/upload/{id}/check");

    let half = size / 2;
    let (status, progress) = send(
        &app,
        "PUT",
        &part_uri(0),
        Body::from(bytes[..half].to_vec()),
    )
    .await;
    assert_eq!(
        (status, progress["received"].as_u64()),
        (StatusCode::OK, Some(half as u64))
    );
    // Not all of it yet: nothing is checked.
    let (status, body) = send(&app, "POST", &check_uri, Body::empty()).await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::CONFLICT, Some("backup.upload_offset"))
    );
    // A part sent twice is refused, and says where the upload stands.
    let (status, body) = send(
        &app,
        "PUT",
        &part_uri(0),
        Body::from(bytes[..half].to_vec()),
    )
    .await;
    assert_eq!(
        (status, body["received"].as_u64()),
        (StatusCode::CONFLICT, Some(half as u64))
    );
    // Nothing past the size given at the start.
    let mut over = bytes[half..].to_vec();
    over.push(0);
    let (status, _) = send(&app, "PUT", &part_uri(half), Body::from(over)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(
        &app,
        "PUT",
        &part_uri(half),
        Body::from(bytes[half..].to_vec()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, preview) = send(&app, "POST", &check_uri, Body::empty()).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["redemptions"], 2);
    assert!(dir.join(STAGING_DIR).join(MANIFEST).is_file(), "staged");
    assert_eq!(uploads(&dir), 0, "the uploaded file is gone once checked");
    assert_eq!(state_of(&dir).await, before, "nothing in use changes");
    // Checked once: the upload is over.
    let (status, _) = send(&app, "POST", &check_uri, Body::empty()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    pool.close().await;
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_damaged_upload_is_refused_and_removed() {
    let (dir, pool) = sample_folder("upload-damaged").await;
    let app = routes(backup_state(&dir, pool.clone(), Arc::default(), |_| None));
    let junk = b"not a backup".to_vec();
    let (_, started) = send(
        &app,
        "POST",
        "/api/admin/backup/restore/upload",
        Body::from(format!(r#"{{"size":{}}}"#, junk.len())),
    )
    .await;
    let id = started["id"].as_str().unwrap().to_owned();
    send(
        &app,
        "PUT",
        &format!("/api/admin/backup/restore/upload/{id}?offset=0"),
        Body::from(junk),
    )
    .await;
    let (status, body) = send(
        &app,
        "POST",
        &format!("/api/admin/backup/restore/upload/{id}/check"),
        Body::empty(),
    )
    .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("backup.invalid"))
    );
    assert_eq!(uploads(&dir), 0);
    assert!(leftovers(&dir).is_empty(), "{:?}", leftovers(&dir));
    pool.close().await;
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_cancelled_replaced_or_cut_off_upload_leaves_nothing() {
    let (dir, pool) = sample_folder("upload-cancel").await;
    let app = routes(backup_state(&dir, pool.clone(), Arc::default(), |_| None));
    let start = || {
        send(
            &app,
            "POST",
            "/api/admin/backup/restore/upload",
            Body::from(r#"{"size":10}"#),
        )
    };
    let (_, first) = start().await;
    let first = first["id"].as_str().unwrap().to_owned();
    send(
        &app,
        "PUT",
        &format!("/api/admin/backup/restore/upload/{first}?offset=0"),
        Body::from(vec![1; 4]),
    )
    .await;
    assert_eq!(uploads(&dir), 1);
    // Starting another drops the first.
    let (_, second) = start().await;
    let second = second["id"].as_str().unwrap().to_owned();
    assert_eq!(uploads(&dir), 1);
    let (status, body) = send(
        &app,
        "PUT",
        &format!("/api/admin/backup/restore/upload/{first}?offset=4"),
        Body::from(vec![1; 4]),
    )
    .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("backup.upload_gone"))
    );
    // Cancelled.
    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/api/admin/backup/restore/upload/{second}"),
        Body::empty(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(uploads(&dir), 0);
    // The app ended mid-way: the next start removes it.
    start().await;
    assert_eq!(uploads(&dir), 1);
    pool.close().await;
    assert!(!apply_pending(&dir, "1.2.3").await.unwrap());
    assert_eq!(uploads(&dir), 0);
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_upload_without_room_or_above_the_cap_is_refused_before_anything_is_written() {
    let (dir, pool) = sample_folder("upload-room").await;
    // 1,000 bytes free: a 600-byte backup needs 1,200 (the upload, then its unpacked copy).
    let app = routes(backup_state(&dir, pool.clone(), Arc::default(), |_| {
        Some(1000)
    }));
    let (status, body) = send(
        &app,
        "POST",
        "/api/admin/backup/restore/upload",
        Body::from(r#"{"size":600}"#),
    )
    .await;
    assert_eq!(status, StatusCode::INSUFFICIENT_STORAGE);
    assert_eq!(
        body,
        serde_json::json!({ "code": "backup.no_space", "needed": 1200 })
    );
    assert_eq!(uploads(&dir), 0);
    let (status, _) = send(
        &app,
        "POST",
        "/api/admin/backup/restore/upload",
        Body::from(r#"{"size":400}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let too_big = format!(r#"{{"size":{}}}"#, MAX_ARCHIVE_BYTES + 1);
    let (status, body) = send(
        &app,
        "POST",
        "/api/admin/backup/restore/upload",
        Body::from(too_big),
    )
    .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::PAYLOAD_TOO_LARGE, Some("backup.too_large"))
    );
    pool.close().await;
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn room_needs_the_bytes_free_and_lets_an_unknown_amount_through() {
    assert!(room(10, Some(10)).is_ok());
    assert!(matches!(room(11, Some(10)), Err(BackupError::NoSpace(11))));
    assert!(room(u64::MAX, None).is_ok());
}

/// The `before-restore-*.stbackup` a restore saved.
fn saved_before_restore(dir: &Path) -> PathBuf {
    let [saved] = fs::read_dir(dir.join(db::BACKUP_DIR))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().contains("before-restore-"))
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    saved
}

fn manifest_of(bytes: &[u8]) -> Manifest {
    let entries = entries(bytes);
    let (_, text) = entries.iter().find(|(n, _)| n == MANIFEST).unwrap();
    serde_json::from_slice(text).unwrap()
}

/// Changes the settings, a database row and the media, so a restore of either kind shows.
async fn change_everything(dir: &Path, pool: &SqlitePool) {
    toolkit_core::settings::update(dir, |s| s.twitch_client_id = "changed".into()).unwrap();
    sqlx::query("DELETE FROM redemption_records WHERE id = 'r1'")
        .execute(pool)
        .await
        .unwrap();
    fs::write(dir.join("sounds/new.mp3"), "new").unwrap();
    fs::remove_dir_all(dir.join("music")).unwrap();
}

async fn restore_bytes(dir: &Path, bytes: Vec<u8>) -> RestorePreview {
    let preview = stage_bytes(dir, bytes).await.unwrap();
    fs::write(dir.join(PENDING), b"id").unwrap();
    assert!(apply_pending(dir, "1.2.4").await.unwrap());
    preview
}

#[tokio::test]
async fn a_data_only_restore_keeps_the_media_and_backs_up_only_the_data() {
    let (dir, pool) = sample_folder("data-only").await;
    let bytes = export_of(&pool, &dir, "1.2.3", &[Content::Data])
        .await
        .unwrap();
    let mut names: Vec<_> = entries(&bytes).into_iter().map(|(n, _)| n).collect();
    names.sort();
    assert_eq!(names, [MANIFEST, SETTINGS, db::DB_FILE]);
    let manifest = manifest_of(&bytes);
    // Older versions read format 1 only: they refuse this one as newer instead of taking it for a whole backup.
    assert_eq!(
        (manifest.format_version, manifest.contents.as_slice()),
        (2, &[Content::Data][..])
    );
    let before = state_of(&dir).await;
    change_everything(&dir, &pool).await;
    let changed = state_of(&dir).await;
    pool.close().await;

    let preview = restore_bytes(&dir, bytes).await;
    assert_eq!(preview.contents, [Content::Data]);
    let after = state_of(&dir).await;
    assert_eq!(
        (&after.settings, &after.rows),
        (&before.settings, &before.rows),
        "the data is back"
    );
    assert_eq!(after.files, changed.files, "the media is untouched");
    let saved = manifest_of(&fs::read(saved_before_restore(&dir)).unwrap());
    assert_eq!(saved.contents, [Content::Data]);
    assert!(
        saved.files.iter().all(|f| !f.path.contains('/')),
        "only the data was backed up"
    );
    assert!(leftovers(&dir).is_empty(), "{:?}", leftovers(&dir));
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_media_only_restore_keeps_the_data_and_backs_up_only_the_media() {
    let (dir, pool) = sample_folder("media-only").await;
    let bytes = export_of(&pool, &dir, "1.2.3", &[Content::Media])
        .await
        .unwrap();
    assert!(
        entries(&bytes)
            .iter()
            .all(|(n, _)| n == MANIFEST || n.contains('/'))
    );
    assert_eq!(manifest_of(&bytes).format_version, 2);
    let before = state_of(&dir).await;
    change_everything(&dir, &pool).await;
    let changed = state_of(&dir).await;
    pool.close().await;

    let preview = restore_bytes(&dir, bytes).await;
    assert_eq!(
        (preview.contents.as_slice(), preview.redemptions),
        (&[Content::Media][..], 0)
    );
    let after = state_of(&dir).await;
    assert_eq!(after.files, before.files, "the media is back");
    assert_eq!(
        (&after.settings, &after.rows),
        (&changed.settings, &changed.rows),
        "the data is untouched"
    );
    let saved = manifest_of(&fs::read(saved_before_restore(&dir)).unwrap());
    assert_eq!(saved.contents, [Content::Media]);
    assert!(
        saved.files.iter().all(|f| f.path.contains('/')),
        "only the media was backed up"
    );
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_backup_from_before_kinds_restores_as_both() {
    let (dir, pool) = sample_folder("format-1").await;
    let whole = export(&pool, &dir, "1.2.3").await.unwrap();
    assert_eq!(
        manifest_of(&whole).format_version,
        1,
        "a whole backup stays readable by older versions"
    );
    let old = with_manifest(&whole, |m| {
        m.as_object_mut().unwrap().remove("contents");
    });
    let preview = stage_bytes(&dir, old).await.unwrap();
    assert_eq!(preview.contents, BOTH);
    pool.close().await;
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn putting_back_a_media_restore_never_touches_the_data() {
    let (dir, pool) = sample_folder("put-back-media").await;
    pool.close().await;
    let before = state_of(&dir).await;
    let (staging, previous) = (dir.join(STAGING_DIR), dir.join(PREVIOUS_DIR));
    fs::create_dir_all(&staging).unwrap();
    swap(&dir, &staging, &previous, "id", &[Content::Media]).unwrap();
    assert!(
        dir.join(SETTINGS).exists() && dir.join(db::DB_FILE).exists(),
        "the data stays in place"
    );
    assert!(!dir.join("sounds").exists());
    put_back(&dir, &previous).unwrap();
    assert_eq!(state_of(&dir).await, before);
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn sizes_count_the_data_and_the_media_apart() {
    let (dir, pool) = sample_folder("sizes").await;
    let app = routes(backup_state(&dir, pool.clone(), Arc::default(), |_| None));
    let (status, sizes) = send(&app, "GET", "/api/admin/backup/sizes", Body::empty()).await;
    assert_eq!(status, StatusCode::OK);
    let media: u64 = ["templates", "sounds", "music", "media", "checkin"]
        .iter()
        .flat_map(|f| walk(&dir.join(f)))
        .sum();
    assert_eq!(sizes["media"].as_u64(), Some(media));
    assert!(sizes["data"].as_u64().unwrap() > fs::metadata(dir.join(SETTINGS)).unwrap().len());
    pool.close().await;
    let _ = fs::remove_dir_all(dir);
}

/// Sizes of every file under `dir`.
fn walk(dir: &Path) -> Vec<u64> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).unwrap().flatten() {
        if entry.file_type().unwrap().is_dir() {
            out.extend(walk(&entry.path()));
        } else {
            out.push(entry.metadata().unwrap().len());
        }
    }
    out
}

#[tokio::test]
async fn a_media_only_backup_of_no_media_still_restores() {
    let (dir, pool) = sample_folder("no-media").await;
    for folder in ["templates", "sounds", "music", "media", "checkin"] {
        let _ = fs::remove_dir_all(dir.join(folder));
    }
    let bytes = export_of(&pool, &dir, "1.2.3", &[Content::Media])
        .await
        .unwrap();
    assert_eq!(entries(&bytes).len(), 1, "only the manifest");
    pool.close().await;
    let preview = restore_bytes(&dir, bytes).await;
    assert_eq!(preview.contents, [Content::Media]);
    assert!(
        dir.join(SETTINGS).exists() && dir.join(db::DB_FILE).exists(),
        "the data stays"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn the_manifest_cap_fits_every_file_the_entry_cap_allows() {
    // The longest path allowed: segments of at most 255 bytes.
    let mut path = String::from("media");
    while path.len() < MAX_PATH_BYTES {
        let n = (MAX_PATH_BYTES - path.len() - 1).min(255);
        path += &format!("/{}", "a".repeat(n));
    }
    assert!(allowed_path(&path) && path.len() == MAX_PATH_BYTES);
    let manifest = |n: usize| Manifest {
        format: FORMAT.into(),
        format_version: FORMAT_VERSION,
        contents: BOTH.to_vec(),
        app_version: "999.999.999".into(),
        schema_version: i64::MAX,
        created_at: "2026-11-22T00:00:00Z".into(),
        records: Records {
            redemptions: i64::MAX,
            checkins: i64::MAX,
        },
        files: (0..n)
            .map(|_| FileEntry {
                path: path.clone(),
                size: u64::MAX,
                sha256: "f".repeat(64),
            })
            .collect(),
    };
    let size = |n| serde_json::to_vec_pretty(&manifest(n)).unwrap().len() as u64;
    let (one, two) = (size(1), size(2));
    let largest = one + (two - one) * (MAX_ENTRIES as u64 - 1);
    assert!(
        largest <= MAX_MANIFEST_BYTES,
        "{largest} > {MAX_MANIFEST_BYTES}"
    );
}

/// Writes a 4 GiB file (zeros, so the backup stays small) and checks it back; slow, so run it by hand.
#[tokio::test]
#[ignore = "writes 4 GiB to disk"]
async fn a_file_of_4_gib_or_more_backs_up_and_checks_out() {
    let (dir, pool) = sample_folder("zip64").await;
    let big = fs::File::create(dir.join("media/big.webm")).unwrap();
    big.set_len(u64::from(u32::MAX) + 1).unwrap();
    drop(big);
    let bytes = export_of(&pool, &dir, "1.2.3", &[Content::Media])
        .await
        .unwrap();
    pool.close().await;
    let file = tempfile::NamedTempFile::new().unwrap();
    fs::write(file.path(), bytes).unwrap();
    let (manifest, _) = validate(file.path()).unwrap();
    let big = manifest
        .files
        .iter()
        .find(|f| f.path == "media/big.webm")
        .unwrap();
    assert_eq!(big.size, u64::from(u32::MAX) + 1);
    let _ = fs::remove_dir_all(dir);
}
