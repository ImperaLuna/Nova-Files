use std::fmt;
use std::path::{Path, PathBuf};

use nova_files_index::{CrawlError, Index, OpenError, crawl};

use crate::places;

const FOLDER: &str = "nova-files";
const INDEX_FILE: &str = "index";
const CHANGES_FILE: &str = "changes";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexPlace {
    pub folder: PathBuf,
}

impl IndexPlace {
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

    pub fn build(&self, root: &Path) -> Result<(), IndexError> {
        let built = crawl(root, || false).map_err(IndexError::Crawl)?;
        built.save(&self.index()).map_err(IndexError::Save)
    }

    pub fn open(&self, root: &Path) -> Result<Index, Unusable> {
        let index = Index::open(&self.index()).map_err(Unusable::Open)?;
        if index.root() == root {
            Ok(index)
        } else {
            Err(Unusable::Stale)
        }
    }
}

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

#[derive(Debug)]
pub enum Unusable {
    Stale,
    Open(OpenError),
}

#[cfg(test)]
mod tests;
