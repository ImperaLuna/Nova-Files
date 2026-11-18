//! Archives (zip and tar, plain or compressed; 7z and rar through the
//! installed 7-Zip), read through the sandboxed helper
//! `nova-files-archiver`, which ships next to the program.
//!
//! The helper opens the archive and parses it locked in; the core only
//! reads its answers (see `wire`) and checks each as outside data. [`Archiver::list`] lists an archive's
//! entries without unpacking any.

mod helper;
mod unpack;
mod wire;

use std::env;
use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use nova_files_api::Packing;

pub use unpack::{Extracting, Flow};
pub use wire::{ArchiveEntry, ArchiveKind};

const HELPER_NAME: &str = "nova-files-archiver";
/// 7-Zip's own program, then the name distributions give it.
const SEVEN_NAMES: [&str; 2] = ["7zz", "7z"];
const SEVEN: &str = "--seven";
const RUNNABLE: u32 = 0o111;

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
    /// Locks each file in a zip or 7z; a 7z's names too.
    pub password: Option<&'a str>,
}

/// Runs the helper, and through it 7-Zip when there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Archiver {
    helper: PathBuf,
    seven: Option<PathBuf>,
}

impl Archiver {
    /// The helper next to the running program, with the 7-Zip on `PATH`;
    /// `None` when the program's own path is unknown.
    pub fn from_env() -> Option<Self> {
        let helper = env::current_exe().ok()?.parent()?.join(HELPER_NAME);
        let seven = seven_on(env::var_os("PATH").as_deref());
        Some(Self { helper, seven })
    }

    /// The helper at `helper`, without 7-Zip.
    #[must_use]
    pub const fn at(helper: PathBuf) -> Self {
        Self {
            helper,
            seven: None,
        }
    }

    /// The same, with the 7-Zip at `seven`.
    #[must_use]
    pub fn with_seven(self, seven: Option<PathBuf>) -> Self {
        Self { seven, ..self }
    }

    /// Whether it reads 7z and makes [`Packing::SevenZip`].
    #[must_use]
    pub const fn has_seven(&self) -> bool {
        self.seven.is_some()
    }

    /// The helper's arguments before its mode.
    fn lead(&self) -> Vec<&OsStr> {
        self.seven
            .iter()
            .flat_map(|seven| [SEVEN.as_ref(), seven.as_os_str()])
            .collect()
    }

    /// Every entry of `archive` whose path stays inside it (see `wire`).
    ///
    /// # Errors
    /// When the archive can't be read; see [`ArchiveFailure`].
    pub fn list(&self, archive: &Path) -> Result<Vec<ArchiveEntry>, ArchiveFailure> {
        self.list_locked(archive, None)
    }

    /// As [`Self::list`], opening an archive whose names are locked with
    /// `password`.
    ///
    /// # Errors
    /// As [`Self::list`].
    pub fn list_locked(
        &self,
        archive: &Path,
        password: Option<&str>,
    ) -> Result<Vec<ArchiveEntry>, ArchiveFailure> {
        let output = helper::list(&self.helper, &self.lead(), archive, password)?;
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
        unpack::extract(self, (archive, &[]), into, password, tell)
    }

    /// As [`Self::extract`], but only the entries at the paths `picked`
    /// inside the archive, each with all inside it, keeping their paths.
    ///
    /// # Errors
    /// As [`Self::extract`].
    pub fn extract_picked(
        &self,
        (archive, picked): (&Path, &[PathBuf]),
        into: &Path,
        password: Option<&str>,
        tell: impl FnMut(Extracting) -> Flow,
    ) -> Result<(), ArchiveFailure> {
        unpack::extract(self, (archive, picked), into, password, tell)
    }

    /// Makes `archive`, a new file, of `sources` (absolute paths), each
    /// under its own name, as `recipe` says. `tell` hears of each entry
    /// packed, and is asked as for [`Self::extract`]. A failure can
    /// leave `archive` part written.
    ///
    /// # Errors
    /// When a source can't be read, `archive` can't be written, or `tell`
    /// stops it; unsupported for a 7z without 7-Zip. See
    /// [`ArchiveFailure`].
    pub fn compress(
        &self,
        sources: &[PathBuf],
        archive: &Path,
        recipe: &Recipe<'_>,
        tell: impl FnMut(Extracting) -> Flow,
    ) -> Result<(), ArchiveFailure> {
        if recipe.packing == Packing::SevenZip && !self.has_seven() {
            return Err(ArchiveFailure::Unsupported);
        }
        unpack::compress(self, sources, archive, recipe, tell)
    }
}

/// The first 7-Zip in the folders `path` lists.
pub fn seven_on(path: Option<&OsStr>) -> Option<PathBuf> {
    let path = path?;
    SEVEN_NAMES.iter().find_map(|name| {
        env::split_paths(path)
            .map(|folder| folder.join(name))
            .find(|program| {
                program.metadata().is_ok_and(|metadata| {
                    metadata.is_file() && metadata.permissions().mode() & RUNNABLE != 0
                })
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_helper_is_next_to_the_program() {
        let next_to = env::current_exe().unwrap().with_file_name(HELPER_NAME);
        let seven = seven_on(env::var_os("PATH").as_deref());
        assert_eq!(
            Archiver::from_env(),
            Some(Archiver::at(next_to).with_seven(seven))
        );
    }

    #[test]
    fn finds_7zip_where_path_says_and_runnable_only() {
        let root = env::temp_dir().join(format!("nova-seven-on-{}", std::process::id()));
        let (first, second) = (root.join("a"), root.join("b"));
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(second.join("7z")).unwrap();
        std::fs::write(first.join("7zz"), b"").unwrap();
        std::fs::write(first.join("7z"), b"").unwrap();
        std::fs::set_permissions(first.join("7z"), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = env::join_paths([&second, &first]).unwrap();
        assert_eq!(seven_on(Some(&path)), Some(first.join("7z")));
        std::fs::set_permissions(first.join("7zz"), std::fs::Permissions::from_mode(0o001))
            .unwrap();
        assert_eq!(seven_on(Some(&path)), Some(first.join("7zz")));
        assert_eq!(seven_on(Some(OsStr::new(""))), None);
        assert_eq!(seven_on(None), None);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_helper_hears_of_7zip_first() {
        let archiver = Archiver::at("h".into());
        assert_eq!(archiver.lead(), Vec::<&OsStr>::new());
        assert!(!archiver.has_seven());
        let archiver = archiver.with_seven(Some("/bin/7zz".into()));
        assert_eq!(
            archiver.lead(),
            [OsStr::new("--seven"), OsStr::new("/bin/7zz")]
        );
        assert!(archiver.has_seven());
    }
}
