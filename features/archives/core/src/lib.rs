#![forbid(unsafe_code)]

mod compress;
mod extract;
mod helper;
pub mod inside;
mod kind;
mod unpack;
mod wire;

use std::env;
use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use nova_files_api::Packing;

pub use compress::{Asked, compress};
pub use extract::{Taking, extract};
pub use kind::Archives;
pub use unpack::{Extracting, Flow};
pub use wire::{ArchiveEntry, ArchiveKind};

const HELPER_NAME: &str = "nova-files-archiver";
const SEVEN_NAMES: [&str; 2] = ["7zz", "7z"];
const SEVEN: &str = "--seven";
const RUNNABLE: u32 = 0o111;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFailure {
    Gone,
    Denied,
    Broken,
    Unsupported,
    NotRead,
    NeedsPassword,
    WrongPassword,
    NoSpace,
    ReadOnly,
    NotWritten,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recipe<'a> {
    pub packing: Packing,
    pub level: u8,
    pub password: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Archiver {
    helper: PathBuf,
    seven: Option<PathBuf>,
}

impl Archiver {
    pub fn from_env() -> Option<Self> {
        let helper = env::current_exe().ok()?.parent()?.join(HELPER_NAME);
        let seven = seven_on(env::var_os("PATH").as_deref());
        Some(Self { helper, seven })
    }

    #[must_use]
    pub const fn at(helper: PathBuf) -> Self {
        Self {
            helper,
            seven: None,
        }
    }

    #[must_use]
    pub fn with_seven(self, seven: Option<PathBuf>) -> Self {
        Self { seven, ..self }
    }

    #[must_use]
    pub const fn has_seven(&self) -> bool {
        self.seven.is_some()
    }

    fn lead(&self) -> Vec<&OsStr> {
        self.seven
            .iter()
            .flat_map(|seven| [SEVEN.as_ref(), seven.as_os_str()])
            .collect()
    }

    pub fn list(&self, archive: &Path) -> Result<Vec<ArchiveEntry>, ArchiveFailure> {
        self.list_locked(archive, None)
    }

    pub fn list_locked(
        &self,
        archive: &Path,
        password: Option<&str>,
    ) -> Result<Vec<ArchiveEntry>, ArchiveFailure> {
        let output = helper::list(&self.helper, &self.lead(), archive, password)?;
        wire::entries(&output).ok_or(ArchiveFailure::Broken)
    }

    pub fn extract(
        &self,
        archive: &Path,
        into: &Path,
        password: Option<&str>,
        tell: impl FnMut(Extracting) -> Flow,
    ) -> Result<(), ArchiveFailure> {
        unpack::extract(self, (archive, &[]), into, password, tell)
    }

    pub fn extract_picked(
        &self,
        (archive, picked): (&Path, &[PathBuf]),
        into: &Path,
        password: Option<&str>,
        tell: impl FnMut(Extracting) -> Flow,
    ) -> Result<(), ArchiveFailure> {
        unpack::extract(self, (archive, picked), into, password, tell)
    }

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
