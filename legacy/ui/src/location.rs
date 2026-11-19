use std::path::{Path, PathBuf};

use nova_files_shell::{self as shell, PageRef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    Start,
    Folder(PathBuf),
    Page(PageRef),
}

impl Location {
    pub fn folder(&self) -> Option<&Path> {
        match self {
            Self::Start | Self::Page(_) => None,
            Self::Folder(path) => Some(path),
        }
    }

    pub fn shell(&self) -> shell::Location {
        match self {
            Self::Folder(path) => shell::Location::Folder(path.clone()),
            Self::Page(page) => shell::Location::Page(page.clone()),
            Self::Start => shell::Location::start(),
        }
    }
}

impl From<&Path> for Location {
    fn from(path: &Path) -> Self {
        Self::Folder(path.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_folder_has_one() {
        let path = Path::new("/home/me");
        assert_eq!(Location::from(path).folder(), Some(path));
        assert_eq!(Location::Start.folder(), None);
    }
}
