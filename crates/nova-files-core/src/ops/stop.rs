//! Why a job stopped before the end: a client cancelled it, or a step
//! failed at a path.

use std::io;
use std::path::{Path, PathBuf};

use nova_files_api::{Failure, Outcome};

use super::control::Cancelled;
use super::failure::failure;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    Cancelled,
    Failed { path: PathBuf, reason: Failure },
}

impl Stop {
    /// The step at `path` failed with `error`.
    pub fn at(path: &Path, error: &io::Error) -> Self {
        Self::because(path, failure(error))
    }

    /// The step at `path` failed for `reason`.
    pub fn because(path: &Path, reason: Failure) -> Self {
        Self::Failed {
            path: path.to_path_buf(),
            reason,
        }
    }

    /// How a job stopped this way ended.
    pub fn outcome(self) -> Outcome {
        match self {
            Self::Cancelled => Outcome::Cancelled,
            Self::Failed { path, reason } => Outcome::Failed { path, reason },
        }
    }
}

impl From<Cancelled> for Stop {
    fn from(_: Cancelled) -> Self {
        Self::Cancelled
    }
}
