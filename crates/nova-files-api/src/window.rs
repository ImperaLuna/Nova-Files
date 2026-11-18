//! A window another program asks for: on a folder or on the Start page,
//! with some of the folder's entries selected, as an app's "Show in
//! folder" wants.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// A new window, as a launch or a D-Bus call asks for it. The default is a
/// window on the Start page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowRequest {
    /// The folder to show, or `None` for the Start page.
    pub folder: Option<PathBuf>,
    /// Names of entries in `folder` to select, in the order asked.
    pub select: Vec<OsString>,
}

impl WindowRequest {
    /// A window on `folder`, with nothing selected.
    pub const fn on(folder: PathBuf) -> Self {
        Self {
            folder: Some(folder),
            select: Vec::new(),
        }
    }

    /// A window on the folder that holds `path`, with `path` selected;
    /// `None` when no folder holds it, as for the root or a path ending in
    /// `..`.
    pub fn showing(path: &Path) -> Option<Self> {
        let name = path.file_name()?;
        Some(Self {
            folder: Some(path.parent()?.to_path_buf()),
            select: vec![name.to_owned()],
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::WindowRequest;

    #[test]
    fn showing_a_path_selects_it_in_its_folder() {
        let request = WindowRequest::showing(Path::new("/home/me/a.txt")).unwrap();
        assert_eq!(request.folder, Some(PathBuf::from("/home/me")));
        assert_eq!(request.select, ["a.txt"]);
    }

    #[test]
    fn no_folder_holds_the_root_or_a_parent_step() {
        assert_eq!(WindowRequest::showing(Path::new("/")), None);
        assert_eq!(WindowRequest::showing(Path::new("/home/..")), None);
    }
}
