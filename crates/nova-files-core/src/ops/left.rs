//! What jobs a crash cut short left behind, found from their journals
//! when the core starts. Each is cleaned up at once: its part files go
//! (half a copy, or, after an exchange, a file the user chose to
//! replace), and a file that stepped aside for a replacement comes back
//! when its replacement never took its name. The job itself then waits,
//! interrupted, until a client resumes or forgets it.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use nova_files_api::{JobId, Operation};

use super::journal::read::{Line, lines};
use super::place::rename_no_replace;

/// The hidden names a job gives its part files and backups. Nothing else
/// is ever removed on a journal's word.
const PART: &str = ".nova-part-";
const BACKUP: &str = ".nova-backup-";

/// A job a crash cut short.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Left {
    pub job: JobId,
    pub operation: Operation,
    /// What it made, which a resume takes as done.
    pub made: HashSet<PathBuf>,
    pub journal: PathBuf,
}

/// The jobs whose journals are in `folder`, each cleaned up. A journal
/// that can't be read is logged and left alone.
pub fn recover(folder: &Path) -> Vec<Left> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut left = Vec::new();
    for entry in entries.flatten() {
        let journal = entry.path();
        let job = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse().ok());
        let read = lines(&journal).map(|lines| (job, operation(&lines), lines));
        match read {
            Ok((Some(job), Some(operation), lines)) => {
                left.push(Left {
                    job: JobId(job),
                    operation,
                    made: clean(&lines),
                    journal,
                });
            }
            Ok(_) => log::warn!("{} names no job it can resume", journal.display()),
            Err(error) => log::warn!("can't read {}: {error}", journal.display()),
        }
    }
    left.sort_by_key(|left| left.job);
    left
}

/// The operation `lines` record: its word, then where to and from what.
fn operation(lines: &[Line]) -> Option<Operation> {
    let sources: Vec<_> = first_paths(lines, "source").collect();
    let into = first_paths(lines, "into").next();
    match (lines.first()?.word.as_str(), into) {
        ("copy", Some(into)) => Some(Operation::Copy { sources, into }),
        ("move", Some(into)) => Some(Operation::Move { sources, into }),
        ("delete", None) => Some(Operation::Delete { paths: sources }),
        _ => None,
    }
}

/// The first path of each of `lines` recorded as `word`.
fn first_paths<'a>(lines: &'a [Line], word: &'a str) -> impl Iterator<Item = PathBuf> + 'a {
    lines
        .iter()
        .filter(move |line| line.word == word)
        .filter_map(|line| line.paths.first().cloned())
}

/// Removes the part files `lines` name, brings back each backup whose
/// replacement never took its name (and removes those whose did), and
/// returns what the job made.
fn clean(lines: &[Line]) -> HashSet<PathBuf> {
    let mut made = HashSet::new();
    for line in lines {
        match (line.word.as_str(), line.paths.as_slice()) {
            ("part", [part]) if hidden(part, PART) => {
                if let Err(error) = fs::remove_file(part)
                    && part.symlink_metadata().is_ok()
                {
                    log::warn!("can't remove {}: {error}", part.display());
                }
            }
            ("backup", [backup, of]) if hidden(backup, BACKUP) => restore(backup, of),
            ("made", [path]) => {
                made.insert(path.clone());
            }
            _ => {}
        }
    }
    made
}

/// Brings `backup` back to `of` if nothing took that name, else removes
/// it: its replacement is in, as the user chose.
fn restore(backup: &Path, of: &Path) {
    if backup.symlink_metadata().is_err() {
        return;
    }
    let restored = if of.symlink_metadata().is_err() {
        rename_no_replace(backup, of)
    } else {
        fs::remove_file(backup)
    };
    if let Err(error) = restored {
        log::warn!("{} is left at {}: {error}", of.display(), backup.display());
    }
}

/// Whether `path`'s name is one of the job's hidden names, starting
/// with `prefix`.
fn hidden(path: &Path, prefix: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(prefix))
}
