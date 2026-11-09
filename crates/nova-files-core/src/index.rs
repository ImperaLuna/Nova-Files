//! Where the file index lives, and building it.
//!
//! The index and its change log sit in `nova-files` under the user's cache
//! folder ([`places::cache_home`]). An index built from another folder than
//! the one asked for is stale, like one from another version: a bench run
//! or a test pointed at a test folder never passes for the home index.

use std::fmt;
use std::path::{Path, PathBuf};

use nova_files_index::{CrawlError, Index, OpenError, crawl};

use crate::places;

/// The folder under the cache folder.
const FOLDER: &str = "nova-files";
const INDEX_FILE: &str = "index";
const CHANGES_FILE: &str = "changes";

/// Where the index and its change log are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexPlace {
    pub folder: PathBuf,
}

impl IndexPlace {
    /// `nova-files` in the user's cache folder, or `None` without one.
    pub fn from_env() -> Option<Self> {
        places::cache_home().map(|cache| Self {
            folder: cache.join(FOLDER),
        })
    }

    pub fn index(&self) -> PathBuf {
        self.folder.join(INDEX_FILE)
    }

    pub fn changes(&self) -> PathBuf {
        self.folder.join(CHANGES_FILE)
    }

    /// Crawls `root` and saves the index here, replacing any index there.
    ///
    /// # Errors
    /// When `root` can't be crawled or the index can't be written.
    pub fn build(&self, root: &Path) -> Result<(), IndexError> {
        let built = crawl(root, || false).map_err(IndexError::Crawl)?;
        built.save(&self.index()).map_err(IndexError::Save)
    }

    /// The saved index, when it was built from `root`.
    ///
    /// # Errors
    /// [`Unusable::Stale`] for an index of another folder, [`Unusable::Open`]
    /// when there is none or it can't be read.
    pub fn open(&self, root: &Path) -> Result<Index, Unusable> {
        let index = Index::open(&self.index()).map_err(Unusable::Open)?;
        if index.root() == root {
            Ok(index)
        } else {
            Err(Unusable::Stale)
        }
    }
}

/// Why building the index failed.
#[derive(Debug)]
pub enum IndexError {
    Crawl(CrawlError),
    Save(std::io::Error),
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Crawl(error) => write!(f, "can't index the folder: {error:?}"),
            Self::Save(error) => write!(f, "can't save the index: {error}"),
        }
    }
}

impl std::error::Error for IndexError {}

/// Why a saved index can't be used, so it has to be built again.
#[derive(Debug)]
pub enum Unusable {
    /// It was built from another folder.
    Stale,
    Open(OpenError),
}

#[cfg(test)]
mod tests;
