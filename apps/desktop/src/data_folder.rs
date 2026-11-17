//! The data directory is the unit of a running copy: settings, logs, cache, WebView2 data and the instance ID
//! all live in it, and only one copy may run per directory. The app ships portable only, so data sits next to
//! the executable unless `--data-dir` points elsewhere.

use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

const INSTANCE_FILE: &str = "instance-id";

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
