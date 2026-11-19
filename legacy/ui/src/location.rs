use std::path::{Path, PathBuf};

use gpui::SharedString;
use nova_files_gathered_api::Gathering;
use nova_files_shell::{self as shell, PageKind, PageRef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    Start,
    Folder(PathBuf),
    Gathered(Gathering),
    Properties(Vec<PathBuf>),
    Page(PageRef),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Builtin {
    Gathered(Gathering),
    Properties(Vec<PathBuf>),
}

impl PageKind for Builtin {
    fn title(&self) -> SharedString {
        SharedString::new_static(match self {
            Self::Gathered(_) => "Gathered",
            Self::Properties(_) => "Properties",
        })
    }
}

impl Location {
    pub fn folder(&self) -> Option<&Path> {
        match self {
            Self::Start | Self::Gathered(_) | Self::Properties(_) | Self::Page(_) => None,
            Self::Folder(path) => Some(path),
        }
    }

    pub fn shell(&self) -> shell::Location {
        let builtin = match self {
            Self::Folder(path) => return shell::Location::Folder(path.clone()),
            Self::Page(page) => return shell::Location::Page(page.clone()),
            Self::Start => return shell::Location::start(),
            Self::Gathered(place) => Builtin::Gathered(*place),
            Self::Properties(paths) => Builtin::Properties(paths.clone()),
        };
        shell::Location::Page(PageRef::new(builtin))
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
