//! Deleting files and folders for good, not to the trash.

#![forbid(unsafe_code)]

use std::path::PathBuf;

use nova_files_api::Operation;

/// Delete each of `paths`, folders with all they hold, for good.
///
/// Nothing goes to the trash. A link goes, never what it points to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteJob {
    pub paths: Vec<PathBuf>,
}

/// The job that deletes `paths` for good.
pub fn delete(paths: Vec<PathBuf>) -> Operation {
    Operation::feature(DeleteJob { paths })
}

/// The word a delete opens its journal with, so a delete left by a crash
/// is known again.
pub mod journal {
    pub const DELETE: &str = "delete";
}
