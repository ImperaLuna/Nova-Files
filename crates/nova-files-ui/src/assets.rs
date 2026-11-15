//! The icons, built into the program so the window never reads them from
//! the disk. GPUI asks for them by path; [`Icon`] names each one. Their
//! shapes are in `svg`; they are drawn as masks, and the UI colors them
//! with theme tokens.

mod svg;

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

/// The icons the UI draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Folder,
    File,
    Home,
    Desktop,
    Documents,
    Downloads,
    Pictures,
    Music,
    Videos,
    Start,
    Drive,
    Back,
    Forward,
    Up,
    SortUp,
    SortDown,
    Search,
    List,
    Tiles,
    Trash,
    Rename,
    Plus,
    Close,
    Chevron,
    Eject,
    Open,
}

impl Icon {
    const ALL: [Self; 26] = [
        Self::Folder,
        Self::File,
        Self::Home,
        Self::Desktop,
        Self::Documents,
        Self::Downloads,
        Self::Pictures,
        Self::Music,
        Self::Videos,
        Self::Start,
        Self::Drive,
        Self::Back,
        Self::Forward,
        Self::Up,
        Self::SortUp,
        Self::SortDown,
        Self::Search,
        Self::List,
        Self::Tiles,
        Self::Trash,
        Self::Rename,
        Self::Plus,
        Self::Close,
        Self::Chevron,
        Self::Eject,
        Self::Open,
    ];

    /// The path GPUI asks the asset source for.
    pub const fn path(self) -> &'static str {
        match self {
            Self::Folder => "icons/folder.svg",
            Self::File => "icons/file.svg",
            Self::Home => "icons/home.svg",
            Self::Desktop => "icons/desktop.svg",
            Self::Documents => "icons/documents.svg",
            Self::Downloads => "icons/downloads.svg",
            Self::Pictures => "icons/pictures.svg",
            Self::Music => "icons/music.svg",
            Self::Videos => "icons/videos.svg",
            Self::Start => "icons/start.svg",
            Self::Drive => "icons/drive.svg",
            Self::Back => "icons/back.svg",
            Self::Forward => "icons/forward.svg",
            Self::Up => "icons/up.svg",
            Self::SortUp => "icons/sort-up.svg",
            Self::SortDown => "icons/sort-down.svg",
            Self::Search => "icons/search.svg",
            Self::List => "icons/list.svg",
            Self::Tiles => "icons/tiles.svg",
            Self::Trash => "icons/trash.svg",
            Self::Rename => "icons/rename.svg",
            Self::Plus => "icons/plus.svg",
            Self::Close => "icons/close.svg",
            Self::Chevron => "icons/chevron.svg",
            Self::Eject => "icons/eject.svg",
            Self::Open => "icons/open.svg",
        }
    }

    const fn bytes(self) -> &'static [u8] {
        let source = match self {
            Self::Folder => svg::FOLDER,
            Self::File => svg::FILE,
            Self::Home => svg::HOME,
            Self::Desktop => svg::DESKTOP,
            Self::Documents => svg::DOCUMENTS,
            Self::Downloads => svg::DOWNLOADS,
            Self::Pictures => svg::PICTURES,
            Self::Music => svg::MUSIC,
            Self::Videos => svg::VIDEOS,
            Self::Start => svg::START,
            Self::Drive => svg::DRIVE,
            Self::Back => svg::BACK,
            Self::Forward => svg::FORWARD,
            Self::Up => svg::UP,
            Self::SortUp => svg::SORT_UP,
            Self::SortDown => svg::SORT_DOWN,
            Self::Search => svg::SEARCH,
            Self::List => svg::LIST,
            Self::Tiles => svg::TILES,
            Self::Trash => svg::TRASH,
            Self::Rename => svg::RENAME,
            Self::Plus => svg::PLUS,
            Self::Close => svg::CLOSE,
            Self::Chevron => svg::CHEVRON,
            Self::Eject => svg::EJECT,
            Self::Open => svg::OPEN,
        };
        source.as_bytes()
    }
}

/// Serves the built-in icons to GPUI.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(Icon::ALL
            .into_iter()
            .find(|icon| icon.path() == path)
            .map(|icon| Cow::Borrowed(icon.bytes())))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(Icon::ALL
            .into_iter()
            .map(Icon::path)
            .filter(|icon| icon.starts_with(path))
            .map(SharedString::from)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_icon_loads_by_its_path() {
        for icon in Icon::ALL {
            let loaded = Assets.load(icon.path()).unwrap().unwrap();
            assert_eq!(&*loaded, icon.bytes());
        }
        assert!(Assets.load("icons/missing.svg").unwrap().is_none());
    }

    #[test]
    fn every_icon_has_its_own_path_and_shape() {
        let paths: HashSet<_> = Icon::ALL.into_iter().map(Icon::path).collect();
        let shapes: HashSet<_> = Icon::ALL.into_iter().map(Icon::bytes).collect();
        assert_eq!(paths.len(), Icon::ALL.len());
        assert_eq!(shapes.len(), Icon::ALL.len());
    }

    #[test]
    fn lists_the_icons_under_a_path() {
        let listed = Assets.list("icons/").unwrap();
        let expected: Vec<SharedString> = Icon::ALL
            .into_iter()
            .map(|icon| icon.path().into())
            .collect();
        assert_eq!(listed, expected);
        assert_eq!(Assets.list("fonts/").unwrap().len(), 0);
    }
}
