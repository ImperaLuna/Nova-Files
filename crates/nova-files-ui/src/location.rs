//! Where a window can be: the Start page, or a folder.

use std::path::{Path, PathBuf};

/// How a path under the home folder is written: `~/Projects`.
pub const HOME_MARK: &str = "~";

/// A place a window shows and its history steps through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// The Start page, where a new window opens.
    Start,
    Folder(PathBuf),
}

impl Location {
    /// The folder, when this is one.
    pub fn folder(&self) -> Option<&Path> {
        match self {
            Self::Start => None,
            Self::Folder(path) => Some(path),
        }
    }
}

impl From<&Path> for Location {
    fn from(path: &Path) -> Self {
        Self::Folder(path.to_owned())
    }
}
