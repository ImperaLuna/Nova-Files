//! Git: the state of the git repositories a window shows, read with the
//! `git` program the system has. Without it, no folder is a repository.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

/// What a window asks the git core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Read the state of each folder in `folders`, in turn: each answer is
    /// an [`Event::Repo`], sent to the client that asked as soon as that
    /// folder is read. Only a folder at the top of a repository (one that
    /// holds `.git`) has a [`Repo`]. A folder is read again whenever its
    /// repository changes (a commit, checkout, stage, fetch or push), until
    /// a later `Repos` from the same client; an empty one stops it.
    Repos(Vec<PathBuf>),
    /// Mark what is inside `folder`, the folder on screen, by its git
    /// state: the answer is [`Event::Marks`], sent again whenever the
    /// folder or its repository changes, until a later `Marks` from the
    /// same client. `None` stops it.
    Marks(Option<PathBuf>),
}

/// What the git core tells a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The state of `folder`, or `None` when it is not the top of a git
    /// repository or git could not read it.
    Repo { folder: PathBuf, repo: Option<Repo> },
    /// What is inside `folder` (see [`Command::Marks`]), or `None` when it
    /// lies in no repository git can read.
    Marks {
        folder: PathBuf,
        marks: Option<Marks>,
    },
}

/// The git state of a folder's entries, and of the repository it is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marks {
    /// The repository's branch and counts; `changed` tells whether
    /// anything inside this folder changed.
    pub repo: Repo,
    /// Each entry of the folder that git sees as not plain committed, by
    /// its name. A folder is [`Mark::Changed`] when anything inside it
    /// changed; an entry missing here is unchanged.
    pub entries: BTreeMap<OsString, Mark>,
}

/// How git sees one entry, from least to most.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mark {
    /// Ignored by a `.gitignore` rule.
    Ignored,
    /// Not tracked yet.
    New,
    /// Edited, staged, deleted inside, renamed or in conflict.
    Changed,
}

/// One repository's state, as `git status` sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub head: Head,
    /// Some file is edited, staged, deleted or new and not ignored.
    pub changed: bool,
    /// Commits on the branch that its upstream lacks, as of the last fetch.
    pub ahead: u32,
    /// Commits on the upstream that the branch lacks, as of the last fetch.
    pub behind: u32,
}

/// What the repository has checked out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    /// A branch, by its name; it may have no commits yet.
    Branch(String),
    /// A commit that no branch points at here, by its short id.
    Detached(String),
}
