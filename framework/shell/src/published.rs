use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{App, Global, SharedString};

use crate::assets::Icon;

pub struct Published<T>(pub T);

impl<T: 'static> Global for Published<T> {}

impl<T: 'static> Published<T> {
    pub fn set(value: T, cx: &mut App) {
        cx.set_global(Self(value));
    }

    pub fn get(cx: &App) -> Option<&T> {
        cx.try_global::<Self>().map(|published| &published.0)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ThumbnailsShown {
    #[default]
    Shown,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resizable {
    Sidebar,
    Part,
    Folder,
    Kind,
    Modified,
    Size,
}

impl Resizable {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sidebar => "sidebar",
            Self::Part => "part",
            Self::Folder => "folder",
            Self::Kind => "type",
            Self::Modified => "modified",
            Self::Size => "size",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Widths(pub BTreeMap<String, u16>);

impl Widths {
    pub fn of(&self, part: Resizable) -> Option<u16> {
        self.0.get(part.name()).copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownFolder {
    pub path: PathBuf,
    pub name: SharedString,
    pub icon: Icon,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnownFolders(Arc<[KnownFolder]>);

impl KnownFolders {
    pub fn new(folders: impl IntoIterator<Item = KnownFolder>) -> Self {
        Self(folders.into_iter().collect())
    }

    pub fn at(&self, path: &Path) -> Option<&KnownFolder> {
        self.0.iter().find(|known| known.path == path)
    }

    pub fn icon_at(&self, path: &Path) -> Icon {
        self.at(path).map_or(Icon::Folder, |known| known.icon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_folder_gives_its_icon_and_others_show_a_plain_folder() {
        let known = KnownFolders::new([KnownFolder {
            path: "/home/me/Music".into(),
            name: "Music".into(),
            icon: Icon::Music,
        }]);

        assert_eq!(known.icon_at(Path::new("/home/me/Music")), Icon::Music);
        assert_eq!(known.icon_at(Path::new("/home/me/Work")), Icon::Folder);
        assert_eq!(
            known
                .at(Path::new("/home/me/Music"))
                .map(|known| &known.name),
            Some(&SharedString::from("Music"))
        );
    }
}
