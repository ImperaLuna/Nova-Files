//! Temp folder trees for the index tests.

#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// A folder under the system temp folder, removed when dropped.
pub struct TempFolder(PathBuf);

impl TempFolder {
    pub fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "nova-files-index-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Makes the files at `paths` (and their folders); a path ending in
    /// `/` is a folder. Each file holds its own path.
    pub fn with(self, paths: &[&str]) -> Self {
        for path in paths {
            let full = self.0.join(path);
            if path.ends_with('/') {
                fs::create_dir_all(&full).unwrap();
            } else {
                fs::create_dir_all(full.parent().unwrap()).unwrap();
                fs::write(&full, path.as_bytes()).unwrap();
            }
        }
        self
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}
