//! A temp folder for the engine's tests, removed when dropped.

use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU32, Ordering};

pub struct TestFolder(PathBuf);

impl TestFolder {
    /// An empty folder named after `test`, so tests running at once never
    /// share one.
    pub fn new(test: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "nova-files-ops-{test}-{}-{}",
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

    /// Writes `text` to the file `name` and returns its path.
    pub fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for TestFolder {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}
