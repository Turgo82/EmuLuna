//! Exclusive native save sessions. Separate from the existing Qt save storage.
use std::{
    fs::{File, OpenOptions},
    path::Path,
};
pub struct SaveSession {
    _file: File,
}
impl SaveSession {
    /// Keep the file and OS lock alive until every core operation has finished.
    /// The lock file stays in place: unlinking it would allow a second inode and
    /// a second writer. The OS releases the lock even after a process crash.
    pub fn acquire(directory: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        let path = directory.join(".native-session.lock");
        if std::fs::symlink_metadata(&path).is_ok_and(|m| !m.file_type().is_file()) {
            return Err("Native save lock is not a regular file".into());
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| e.to_string())?;
        file.try_lock().map_err(|_| {
            "This game's native saves are already in use. Close its other window first.".to_owned()
        })?;
        Ok(Self { _file: file })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_save_lock_excludes_other_sessions_without_truncating_progress() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("saves");
        let first = SaveSession::acquire(&path).unwrap();
        std::fs::write(path.join("battery.srm"), b"progress").unwrap();
        assert!(SaveSession::acquire(&path).is_err());
        drop(first);
        let second = SaveSession::acquire(&path).unwrap();
        assert_eq!(
            std::fs::read(path.join("battery.srm")).unwrap(),
            b"progress"
        );
        drop(second);
    }
}
