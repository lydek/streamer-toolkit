//! The data directory is the unit of a running copy: settings, logs, cache, WebView2 data and the instance ID
//! all live in it, and only one copy may run per directory. The app ships portable only, so data sits next to
//! the executable unless `--data-dir` points elsewhere.

use std::{
    env, fs, io,
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
/// the folder when it is moved; a copied folder is the same instance and cannot run next to the original.
pub fn instance_id(dir: &Path) -> io::Result<String> {
    let path = dir.join(INSTANCE_FILE);
    if let Ok(text) = fs::read_to_string(&path) {
        let id = text.trim();
        if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(id.to_owned());
        }
        tracing::warn!(
            code = "data_folder.instance_id_invalid",
            "replacing an unreadable instance ID; sign in again"
        );
    }
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    fs::write(&path, &id)?;
    Ok(id)
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
}
