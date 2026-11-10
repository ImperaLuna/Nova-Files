//! Open With: which apps can open some files, opening them in one of
//! those, and making one the default for their type.
//!
//! A client asks with [`Command::OpenWith`]. The apps come from the same
//! lists every desktop reads (`mimeapps.list` and `mimeinfo.cache`), so the
//! default set here is the default in other file managers too.
//!
//! [`Command::OpenWith`]: crate::Command::OpenWith

use std::path::PathBuf;

/// Names one Open With request. The client picks it; the core echoes it
/// in the [`OpenWithAnswer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OpenWithId(pub u64);

/// An installed app that opens a type of file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct App {
    /// Its desktop ID (`org.gnome.Loupe.desktop`): what
    /// [`OpenWith::Launch`] and [`OpenWith::SetDefault`] name it by.
    pub id: String,
    pub name: String,
    /// Its icon's name in the icon theme, or a path to one.
    pub icon: Option<String>,
    /// It is the app a plain open of the files uses. Only set when the
    /// files all have the same type.
    pub default: bool,
}

/// One Open With request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenWith {
    /// Which apps open all of `paths`. The answer is
    /// [`OpenWithAnswer::Apps`] with the same `id`.
    Apps { id: OpenWithId, paths: Vec<PathBuf> },
    /// Open `paths` in the app `app`. `activation` is the caller's
    /// activation token, so the app may take focus. The answer is
    /// [`OpenWithAnswer::Launched`] with the same `id`.
    Launch {
        id: OpenWithId,
        paths: Vec<PathBuf>,
        app: String,
        activation: Option<String>,
    },
    /// Make `app` the default for the MIME type `mime`, for this user. The
    /// next [`OpenWith::Apps`] shows it. Nothing answers.
    SetDefault { mime: String, app: String },
}

/// The core's answer to an Open With request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenWithAnswer {
    /// The apps that open every file of [`OpenWith::Apps`] `id`, the
    /// default first, then the rest best first. `mime` is the type the
    /// files all have (`inode/directory` for folders), the one
    /// [`OpenWith::SetDefault`] takes; `None` for files of different
    /// types, whose apps are those for a type they are all kinds of.
    Apps {
        id: OpenWithId,
        mime: Option<String>,
        apps: Vec<App>,
    },
    /// Whether [`OpenWith::Launch`] `id` started the app: `false` when
    /// sandboxed, for an app that runs in a terminal or can't take files,
    /// or when its program couldn't start.
    Launched { id: OpenWithId, started: bool },
}
