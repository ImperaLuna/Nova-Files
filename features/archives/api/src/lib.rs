#![forbid(unsafe_code)]

use std::path::PathBuf;

/// What a window asks the archives core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Which formats can be made here. The answer is [`Event::Formats`],
    /// sent to the client that asked.
    Formats,
    /// Whether `folder` lies inside an archive opened as a folder. The
    /// answer is [`Event::ArchiveOf`], sent to the client that asked.
    ArchiveOf { folder: PathBuf },
}

/// What the archives core tells a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Whether an installed 7-Zip lets this system make, list and unpack
    /// 7z, and list and unpack rar. Zip and tar are always there.
    Formats { seven: bool },
    /// `folder` lies inside the archive file `archive`, or in no archive
    /// when `None`. What is inside an archive can be read but not changed.
    ArchiveOf {
        folder: PathBuf,
        archive: Option<PathBuf>,
    },
}
