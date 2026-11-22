#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

use std::fs;
use std::path::Path;

use nova_files_test_folder::TestFolder;

pub struct TempFolder(TestFolder);

impl TempFolder {
    pub fn new() -> Self {
        Self(TestFolder::new("index-test"))
    }

    pub fn path(&self) -> &Path {
        self.0.path()
    }

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
