//! An entry's icon as the core finds it: a file of the system icon
//! theme, or, when no theme has one, the kind of file it is, so the window
//! draws a built-in icon and no row is ever blank. Folders always take the
//! window's own icon.

use std::path::PathBuf;
use std::sync::Arc;

/// The side, in device pixels, the core picks theme icons for: a row's
/// icon at twice the scale. The window shrinks a bigger one to the size
/// it draws.
pub const FILE_ICON_PIXELS: u32 = 48;

/// How an entry's icon is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileIcon {
    /// A file of the system icon theme. Entries of one type share it.
    Theme(Arc<ThemeIcon>),
    /// The window's own icon for this kind.
    BuiltIn(IconKind),
}

/// An icon file from the theme, read whole.
#[derive(Debug, PartialEq, Eq)]
pub struct ThemeIcon {
    /// Where it was found; two icons with the same path are the same icon.
    pub path: PathBuf,
    pub format: IconFormat,
    pub bytes: Arc<[u8]>,
}

/// The formats the icon theme spec allows that the window draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconFormat {
    Svg,
    Png,
}

/// The kinds the built-in icons tell apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconKind {
    Folder,
    /// Any file no other kind fits.
    File,
    Text,
    Picture,
    Video,
    Audio,
    Archive,
    Program,
}
