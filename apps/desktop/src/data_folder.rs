//! The data directory is the unit of a running copy: settings, logs, cache, WebView2 data and the instance ID
//! all live in it, and only one copy may run per directory. The app ships portable only, so data sits next to
//! the executable unless `--data-dir` points elsewhere.

use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

const INSTANCE_FILE: &str = "instance-id";
/// Written by dev builds into folders they create. Scripts that prepare a folder before starting a dev build
/// with `--data-dir` (e.g. `pnpm dev:fake`) must create this file too.
pub const DEV_MARKER: &str = "dev-folder";
/// Lets a dev build open a non-empty folder it did not create (e.g. a copy of release data). Never marks it.
pub const ALLOW_RELEASE_DATA: &str = "--allow-release-data";

/// What a dev build found in the data folder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FolderState {
    Missing,
    Empty,
    /// Has the dev marker.
    Marked,
    /// Non-empty and unmarked: made by a release build, or by hand.
    Foreign,
}

#[derive(Debug, PartialEq)]
pub enum DevAccess {
    Allow,
    /// Allow, and write the dev marker first.
    Mark,
    Refuse,
}

/// Keeps dev builds out of release data: in-progress migrations applied there make the release build refuse the
/// database. Release builds are always allowed and never look at the folder (`state` is not called).
pub fn dev_access(
    is_dev: bool,
    explicit_dir: bool,
    allow_release_data: bool,
    state: impl FnOnce() -> io::Result<FolderState>,
) -> io::Result<DevAccess> {
    if !is_dev {
        return Ok(DevAccess::Allow);
    }
    Ok(match state()? {
        FolderState::Marked => DevAccess::Allow,
        FolderState::Missing | FolderState::Empty => DevAccess::Mark,
        // The default folder next to a dev executable (`target\debug\data`) is a dev folder by definition.
        FolderState::Foreign if !explicit_dir => DevAccess::Mark,
        FolderState::Foreign if allow_release_data => DevAccess::Allow,
        FolderState::Foreign => DevAccess::Refuse,
    })
}

/// Only a missing folder counts as missing; any other read error is returned, never taken for empty.
pub fn folder_state(dir: &Path) -> io::Result<FolderState> {
    let mut entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(FolderState::Missing),
        Err(e) => return Err(e),
    };
    if dir.join(DEV_MARKER).is_file() {
        return Ok(FolderState::Marked);
    }
    Ok(match entries.next() {
        None => FolderState::Empty,
        Some(entry) => {
            entry?;
            FolderState::Foreign
        }
    })
}

pub fn mark_dev(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    fs::write(dir.join(DEV_MARKER), b"")
}

/// Whether `flag` is on the command line.
pub fn has_flag(flag: &str) -> bool {
    env::args().any(|a| a == flag)
}

/// `--data-dir <path>` on the command line, otherwise the `data` folder next to the executable.
pub fn resolve() -> io::Result<PathBuf> {
    let args: Vec<String> = env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--data-dir") {
        let dir = args.get(i + 1).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "--data-dir needs a path")
        })?;
        return std::path::absolute(dir);
    }
    let exe_dir = env::current_exe()?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "executable has no folder"))?;
    Ok(exe_dir.join("data"))
}

/// Creates the folder and proves it is writable; a copy never falls back to another folder silently.
pub fn ensure_writable(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let probe = dir.join(".write-test");
    fs::write(&probe, b"ok")?;
    fs::remove_file(probe)
}

/// A random ID kept in the folder. It names the credential entry and the single-instance lock, so it follows
/// the folder when it is moved; a copied folder is the same instance and cannot run next to the original. A new
/// ID is made only when the file does not exist: any other read error is returned, since a new ID would lose the
/// stored sign-ins and let a second copy run.
pub fn instance_id(dir: &Path) -> io::Result<String> {
    let path = dir.join(INSTANCE_FILE);
    match fs::read_to_string(&path) {
        Ok(text) => {
            if let Some(id) = valid(&text) {
                return Ok(id);
            }
            tracing::warn!(
                code = "data_folder.instance_id_invalid",
                "replacing an unreadable instance ID; sign in again"
            );
            let id = new_id()?;
            fs::write(&path, &id)?;
            return Ok(id);
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let id = new_id()?;
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => {
            file.write_all(id.as_bytes())?;
            file.sync_all()?;
            Ok(id)
        }
        // Another copy started at the same moment and created it first: use its ID once written.
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            for _ in 0..50 {
                if let Some(id) = valid(&fs::read_to_string(&path)?) {
                    return Ok(id);
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "instance-id is being written by another copy",
            ))
        }
        Err(e) => Err(e),
    }
}

fn valid(text: &str) -> Option<String> {
    let id = text.trim();
    (id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())).then(|| id.to_owned())
}

fn new_id() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn access(explicit: bool, allow: bool, state: FolderState) -> DevAccess {
        dev_access(true, explicit, allow, || Ok(state)).unwrap()
    }

    #[test]
    fn dev_builds_refuse_a_foreign_folder_given_with_data_dir() {
        assert_eq!(access(true, false, FolderState::Foreign), DevAccess::Refuse);
        assert_eq!(access(true, true, FolderState::Foreign), DevAccess::Allow);
    }

    #[test]
    fn dev_builds_mark_new_and_default_folders() {
        for explicit in [true, false] {
            assert_eq!(
                access(explicit, false, FolderState::Missing),
                DevAccess::Mark
            );
            assert_eq!(access(explicit, false, FolderState::Empty), DevAccess::Mark);
            assert_eq!(
                access(explicit, false, FolderState::Marked),
                DevAccess::Allow
            );
        }
        assert_eq!(access(false, false, FolderState::Foreign), DevAccess::Mark);
    }

    #[test]
    fn release_builds_never_look_at_the_folder() {
        let got = dev_access(false, true, false, || -> io::Result<FolderState> {
            panic!("release builds must not read the folder")
        });
        assert_eq!(got.unwrap(), DevAccess::Allow);
    }

    #[test]
    fn an_unreadable_folder_is_an_error_not_empty() {
        let got = dev_access(true, true, false, || {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        });
        assert!(got.is_err());
    }

    #[test]
    fn folder_state_reads_missing_empty_marked_and_foreign() {
        let dir = env::temp_dir().join(format!("st-dev-marker-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(folder_state(&dir).unwrap(), FolderState::Missing);
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(folder_state(&dir).unwrap(), FolderState::Empty);
        fs::write(dir.join("settings.json"), "{}").unwrap();
        assert_eq!(folder_state(&dir).unwrap(), FolderState::Foreign);
        mark_dev(&dir).unwrap();
        assert_eq!(folder_state(&dir).unwrap(), FolderState::Marked);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_where_the_folder_should_be_is_an_error() {
        let path = env::temp_dir().join(format!("st-dev-marker-file-{}", std::process::id()));
        fs::write(&path, "x").unwrap();
        assert!(folder_state(&path).is_err());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn instance_id_is_created_once_and_kept() {
        let dir = env::temp_dir().join(format!("st-instance-{}", std::process::id()));
        ensure_writable(&dir).unwrap();
        let first = instance_id(&dir).unwrap();
        assert_eq!(first.len(), 32);
        assert_eq!(instance_id(&dir).unwrap(), first);

        fs::write(dir.join(INSTANCE_FILE), "garbage").unwrap();
        let replaced = instance_id(&dir).unwrap();
        assert_ne!(replaced, first);
        assert_eq!(instance_id(&dir).unwrap(), replaced);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_read_error_other_than_missing_never_makes_a_new_id() {
        let dir = env::temp_dir().join(format!("st-instance-unreadable-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        // Unreadable as a file, but there.
        fs::create_dir_all(dir.join(INSTANCE_FILE)).unwrap();
        assert!(instance_id(&dir).is_err());
        assert!(dir.join(INSTANCE_FILE).is_dir(), "left as it was");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn copies_starting_together_share_one_id() {
        let dir = env::temp_dir().join(format!("st-instance-race-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        ensure_writable(&dir).unwrap();
        let start = std::sync::Arc::new(std::sync::Barrier::new(8));
        let ids: Vec<String> = (0..8)
            .map(|_| {
                let (dir, start) = (dir.clone(), start.clone());
                std::thread::spawn(move || {
                    start.wait();
                    instance_id(&dir).unwrap()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect();
        assert!(ids.iter().all(|id| *id == ids[0]), "{ids:?}");
        assert_eq!(fs::read_to_string(dir.join(INSTANCE_FILE)).unwrap(), ids[0]);
        let _ = fs::remove_dir_all(dir);
    }
}
