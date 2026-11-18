//! Opening files: in the user's default app for their type, or in one
//! picked from the apps that open them (Open With), and making one the
//! default for their type.
//!
//! A client opens a file with [`Command::OpenFile`] and asks about apps
//! with [`Command::OpenWith`]. The apps come from the same lists every
//! desktop reads (`mimeapps.list` and `mimeinfo.cache`), so the default set
//! here is the default in other file managers too.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

/// A request from a client to the open feature's core half.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Open the file at `path` in the user's default app for its type.
    /// `activation` is the caller's activation token, so the app may take
    /// focus. The answer is [`Event::Opened`] or [`Event::NotOpened`], when
    /// the client should ask the desktop portal instead.
    OpenFile {
        path: PathBuf,
        activation: Option<String>,
    },
    /// List the apps that open some files, open them in one, or make one
    /// the default (see [`OpenWith`]).
    OpenWith(OpenWith),
}

/// Something the open feature's core half tells one client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// [`Command::OpenFile`]'s app has started on `path`.
    Opened { path: PathBuf },
    /// [`Command::OpenFile`] started nothing for `path`, for `reason`.
    NotOpened { path: PathBuf, reason: NotOpened },
    /// The answer to a [`Command::OpenWith`] request.
    OpenWith(OpenWithAnswer),
}

/// Why [`Command::OpenFile`] started no app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotOpened {
    /// Inside a sandbox, so the desktop portal must do it.
    Sandboxed,
    /// No installed app is the default for the file's type, or its
    /// `Exec` line can't take a file.
    NoApp,
    /// The app's program couldn't start.
    Failed,
}

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

/// Takes a file out of the archive it lies in, so it can be opened. The
/// archives feature fills it; the open feature asks it for any path that
/// isn't on disk.
pub trait TakeOut: Send + Sync {
    /// A copy of the file at `path` taken out of its archive, `None` when
    /// `path` lies in no archive, or [`NotOpened::Failed`] when the archive
    /// gave nothing.
    fn take_out(&self, path: &Path) -> Option<Result<PathBuf, NotOpened>>;
}
