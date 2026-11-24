//! Git: the state of the git repositories a window shows, read with the
//! `git` program the system has. Without it, no folder is a repository.

#![forbid(unsafe_code)]

use std::path::PathBuf;

/// What a window asks the git core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Read the state of each folder in `folders`, in turn: each answer is
    /// an [`Event::Repo`], sent to the client that asked as soon as that
    /// folder is read. Only a folder at the top of a repository (one that
    /// holds `.git`) has a [`Repo`]. A later `Repos` from the same client
    /// drops what is left of this one.
    Repos(Vec<PathBuf>),
}

/// What the git core tells a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The state of `folder`, or `None` when it is not the top of a git
    /// repository or git could not read it.
    Repo { folder: PathBuf, repo: Option<Repo> },
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
