#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU32, Ordering};

pub struct TestFolder(PathBuf);

impl TestFolder {
    pub fn new(test: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "nova-files-{test}-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        fs::remove_dir_all(&path).ok();
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, name: impl AsRef<Path>) -> PathBuf {
        self.0.join(name)
    }

    pub fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }
}

const OWNER_ALL: u32 = 0o700;

fn open_up(folder: &Path) {
    fs::set_permissions(folder, fs::Permissions::from_mode(OWNER_ALL)).ok();
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            open_up(&entry.path());
        }
    }
}

impl Drop for TestFolder {
    fn drop(&mut self) {
        if fs::remove_dir_all(&self.0).is_err() {
            open_up(&self.0);
            fs::remove_dir_all(&self.0).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locked_folder_inside_goes_too() {
        let folder = TestFolder::new("locked-inside");
        let locked = folder.join("locked");
        fs::create_dir_all(locked.join("deeper")).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let path = folder.path().to_owned();
        drop(folder);
        assert!(!path.exists());
    }
}
