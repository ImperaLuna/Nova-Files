use std::path::{Path, PathBuf};

use nova_files_api::Gathering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    Start,
    Folder(PathBuf),
    Gathered(Gathering),
    Properties(Vec<PathBuf>),
}

impl Location {
    pub fn folder(&self) -> Option<&Path> {
        match self {
            Self::Start | Self::Gathered(_) | Self::Properties(_) => None,
            Self::Folder(path) => Some(path),
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
        assert_eq!(Location::Gathered(Gathering::Games).folder(), None);
        assert_eq!(Location::Properties(vec![path.to_owned()]).folder(), None);
    }
}
