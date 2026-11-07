//! The icons, built into the program so the window never reads them from
//! the disk. GPUI asks for them by path; [`Icon`] names each one. They are
//! drawn as masks, so the stroke color here doesn't matter: the UI colors
//! them with theme tokens.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

const FOLDER_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.6" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>"##;
const FILE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.6" stroke-linejoin="round"><path d="M7 3h7l5 5v11a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z"/><path d="M14 3v5h5"/></svg>"##;
const UP_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M5 12l7-7 7 7"/></svg>"##;

/// The icons the UI draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Folder,
    File,
    Up,
}

impl Icon {
    const ALL: [Self; 3] = [Self::Folder, Self::File, Self::Up];

    /// The path GPUI asks the asset source for.
    pub const fn path(self) -> &'static str {
        match self {
            Self::Folder => "icons/folder.svg",
            Self::File => "icons/file.svg",
            Self::Up => "icons/up.svg",
        }
    }

    const fn bytes(self) -> &'static [u8] {
        match self {
            Self::Folder => FOLDER_SVG.as_bytes(),
            Self::File => FILE_SVG.as_bytes(),
            Self::Up => UP_SVG.as_bytes(),
        }
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
    use super::*;

    #[test]
    fn every_icon_loads_by_its_path() {
        for icon in Icon::ALL {
            let loaded = Assets.load(icon.path()).unwrap().unwrap();
            assert_eq!(&*loaded, icon.bytes());
        }
        assert!(Assets.load("icons/missing.svg").unwrap().is_none());
    }
}
