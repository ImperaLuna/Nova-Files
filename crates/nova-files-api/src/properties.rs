//! What a file or folder's properties say about it: what it is, how big,
//! when it changed, who owns it and who may do what with it.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::SystemTime;

/// Names one properties request. The client picks it and the core echoes
/// it, so a slow answer to an older request can't pass for a newer one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PropertiesId(pub u64);

/// What a path is. Unlike a listing's [`EntryKind`](crate::EntryKind), a
/// link is a link: its properties are its own, not its target's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathKind {
    Folder,
    File,
    /// A symbolic link, and the path it holds, as written (it may be
    /// relative to the link's folder, or lead nowhere).
    Link {
        target: PathBuf,
    },
    /// A device, pipe or socket.
    Special,
}

/// A user or group: its number, and its name when the system has one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Account {
    pub id: u32,
    pub name: Option<OsString>,
}

/// A path's properties, answering [`Command::Properties`](crate::Command::Properties).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Properties {
    pub path: PathBuf,
    pub kind: PathKind,
    /// Its own length in bytes. For a folder, the folder entry's own, not
    /// what it holds: a size scan adds that up.
    pub bytes: u64,
    /// What it takes on the drive, in bytes.
    pub on_disk: u64,
    /// When its contents last changed.
    pub modified: Option<SystemTime>,
    /// When it was last read, as far as the drive keeps track.
    pub accessed: Option<SystemTime>,
    /// When its contents, name, owner or permissions last changed.
    pub changed: Option<SystemTime>,
    /// When it was made; `None` where the drive doesn't keep it.
    pub created: Option<SystemTime>,
    /// Its permission bits, with the set-user, set-group and sticky bits:
    /// `0o7777` at most.
    pub mode: u32,
    pub owner: Account,
    pub group: Account,
    /// Whether the user may change its permissions and group: they own
    /// it, or are the superuser (which a network drive may still refuse).
    /// Never for a link, which keeps no permissions of its own.
    pub may_change: bool,
    /// The groups the user is in, which they may give what they own.
    pub groups: Vec<Account>,
    /// Whether it is on a drive mounted read-only, where nothing changes;
    /// `None` when the drive couldn't say.
    pub read_only: Option<bool>,
}
