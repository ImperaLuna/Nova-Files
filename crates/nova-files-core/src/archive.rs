//! Archives (zip and tar, plain or compressed), read through the sandboxed
//! helper `nova-files-archiver`, which ships next to the program.
//!
//! The helper opens the archive and parses it locked in; the core only
//! reads its answers (see `wire`) and checks each as outside data. [`Archiver::list`] lists an archive's
//! entries without unpacking any.

mod helper;
mod unpack;
mod wire;

use std::env;
use std::path::{Path, PathBuf};

use nova_files_api::Packing;

pub use unpack::{Extracting, Flow};
pub use wire::{ArchiveEntry, ArchiveKind};

const HELPER_NAME: &str = "nova-files-archiver";

/// Why an archive couldn't be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFailure {
    /// No file at that path.
    Gone,
    /// The user may not read it.
    Denied,
    /// Damaged, cut short, or the helper's answer isn't its layout.
    Broken,
    /// Not an archive the helper reads.
    Unsupported,
    /// The helper couldn't run, or ran out of time.
    NotRead,
    /// An entry is locked and no password was given.
    NeedsPassword,
    /// The password given doesn't open an entry.
    WrongPassword,
    /// The drive it unpacks to is full.
    NoSpace,
    /// The drive it unpacks to is mounted read-only.
    ReadOnly,
    /// Writing where it unpacks failed for another reason.
    NotWritten,
    /// The caller stopped it.
    Cancelled,
}

/// How to make an archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recipe<'a> {
    pub packing: Packing,
    /// From 1, fastest, to 9, smallest.
    pub level: u8,
    /// Locks each file in a zip.
    pub password: Option<&'a str>,
}

/// Runs the helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Archiver {
    helper: PathBuf,
}

impl Archiver {
    /// The helper next to the running program; `None` when the program's
    /// own path is unknown.
    pub fn from_env() -> Option<Self> {
        let helper = env::current_exe().ok()?.parent()?.join(HELPER_NAME);
        Some(Self { helper })
    }

    /// The helper at `helper`.
    #[must_use]
    pub const fn at(helper: PathBuf) -> Self {
        Self { helper }
    }

    /// Every entry of `archive` whose path stays inside it (see `wire`).
    ///
    /// # Errors
    /// When the archive can't be read; see [`ArchiveFailure`].
    pub fn list(&self, archive: &Path) -> Result<Vec<ArchiveEntry>, ArchiveFailure> {
        let output = helper::list(&self.helper, archive)?;
        wire::entries(&output).ok_or(ArchiveFailure::Broken)
    }

    /// Unpacks `archive` into the folder `into`, which the helper may
    /// write inside and nowhere else, opening locked entries with
    /// `password`. `tell` hears of each entry unpacked, and is asked
    /// at least every tenth of a second whether to go on. What was
    /// unpacked before a failure stays in `into`.
    ///
    /// # Errors
    /// When the archive can't be read, `into` can't be written, or `tell`
    /// stops it; see [`ArchiveFailure`].
    pub fn extract(
        &self,
        archive: &Path,
        into: &Path,
        password: Option<&str>,
        tell: impl FnMut(Extracting) -> Flow,
    ) -> Result<(), ArchiveFailure> {
        unpack::extract(&self.helper, archive, into, password, tell)
    }

    /// Makes `archive`, a new file, of `sources` (absolute paths), each
    /// under its own name, as `recipe` says. `tell` hears of each entry
    /// packed, and is asked as for [`Self::extract`]. A failure can
    /// leave `archive` part written.
    ///
    /// # Errors
    /// When a source can't be read, `archive` can't be written, or `tell`
    /// stops it; see [`ArchiveFailure`].
    pub fn compress(
        &self,
        sources: &[PathBuf],
        archive: &Path,
        recipe: &Recipe<'_>,
        tell: impl FnMut(Extracting) -> Flow,
    ) -> Result<(), ArchiveFailure> {
        unpack::compress(&self.helper, sources, archive, recipe, tell)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_helper_is_next_to_the_program() {
        let next_to = env::current_exe().unwrap().with_file_name(HELPER_NAME);
        assert_eq!(Archiver::from_env(), Some(Archiver::at(next_to)));
    }
}
