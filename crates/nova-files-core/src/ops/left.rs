//! What jobs a crash cut short left behind, found from their journals
//! when the core starts. Each is cleaned up at once: its part files go
//! (half a copy, or, after an exchange, a file the user chose to
//! replace), and a file that stepped aside for a replacement comes back
//! when its replacement never took its name. The job itself then waits,
//! interrupted, until a client resumes or forgets it.
//!
//! A journal another core holds belongs to a job still running there,
//! and is left alone. One that is read is held from then on, until its
//! job resumes, is forgotten, or the core stops.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use nova_files_api::{JobId, Operation};

use super::journal::read::{Line, read};
use super::journal::{BACKUP_PREFIX, PART_PREFIX, hold, word};
use super::place::rename_no_replace;

/// A job a crash cut short.
#[derive(Debug)]
pub struct Left {
    pub job: JobId,
    pub operation: Operation,
    pub done: Done,
    pub journal: PathBuf,
    /// The journal, held.
    pub file: File,
    /// How many of the journal's bytes are whole lines.
    pub whole: u64,
}

/// What a job did before a crash cut it short, which a resume takes as
/// done.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Done {
    /// What it made, by its source, at where it went.
    pub made: HashMap<PathBuf, PathBuf>,
    /// For a delete, what it planned to remove, when it got that far.
    pub planned: Option<HashSet<PathBuf>>,
}

/// The jobs whose journals are in `folder`, each cleaned up. A journal
/// that can't be read is logged and left alone; one whose job can't be
/// resumed is cleaned up and removed.
pub fn recover(folder: &Path) -> Vec<Left> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut left: Vec<_> = entries
        .flatten()
        .filter_map(|entry| look_at(entry.path(), entry.file_name().to_str()))
        .collect();
    left.sort_by_key(|left| left.job);
    left
}

/// The job the journal at `journal`, named `name`, left, cleaned up.
fn look_at(journal: PathBuf, name: Option<&str>) -> Option<Left> {
    // Held already (it would block): its job runs in another core.
    let file = open(&journal)
        .inspect_err(|error| log::info!("{} is left alone: {error}", journal.display()))
        .ok()?;
    let written = read(&file, &journal)
        .inspect_err(|error| log::warn!("can't read {}: {error}", journal.display()))
        .ok()?;
    let done = clean(&written.lines);
    let job = name.and_then(|name| name.parse().ok());
    let (Some(job), Some(operation)) = (job, operation(&written.lines)) else {
        log::warn!("{} names no job it can resume", journal.display());
        if let Err(error) = fs::remove_file(&journal) {
            log::warn!("can't remove {}: {error}", journal.display());
        }
        return None;
    };
    Some(Left {
        job: JobId(job),
        operation,
        done,
        journal,
        file,
        whole: written.whole,
    })
}

/// Opens the journal at `path` and holds it.
fn open(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new().read(true).append(true).open(path)?;
    hold(&file)?;
    Ok(file)
}

/// The operation `lines` record: its word, then where to and from what.
fn operation(lines: &[Line]) -> Option<Operation> {
    let sources: Vec<_> = first_paths(lines, word::SOURCE).collect();
    let into = first_paths(lines, word::INTO).next();
    match (lines.first()?.word.as_str(), into) {
        (word::COPY, Some(into)) => Some(Operation::Copy { sources, into }),
        (word::MOVE, Some(into)) => Some(Operation::Move { sources, into }),
        (word::DELETE, None) => Some(Operation::Delete { paths: sources }),
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
/// returns what the job did.
fn clean(lines: &[Line]) -> Done {
    let mut done = Done::default();
    // The names the job's own copies took.
    let mut placed = HashSet::new();
    let mut planned = HashSet::new();
    let mut whole_plan = false;
    for line in lines {
        match (line.word.as_str(), line.paths.as_slice()) {
            (word::MADE, [made, from]) => {
                done.made.insert(from.clone(), made.clone());
                placed.insert(made.clone());
            }
            (word::MOVED, [_, to]) => {
                placed.insert(to.clone());
            }
            (word::PLANNED, [path]) => {
                planned.insert(path.clone());
            }
            // Only a plan recorded whole is kept to: a crash while it was
            // written leaves just its start.
            (word::PLAN_DONE, []) => whole_plan = true,
            _ => {}
        }
    }
    if whole_plan {
        done.planned = Some(planned);
    }
    for line in lines {
        match (line.word.as_str(), line.paths.as_slice()) {
            (word::PART, [part]) if hidden(part, PART_PREFIX) => remove_part(part),
            (word::BACKUP, [backup, of]) if hidden(backup, BACKUP_PREFIX) => {
                restore(backup, of, placed.contains(of));
            }
            _ => {}
        }
    }
    done
}

fn remove_part(part: &Path) {
    if let Err(error) = fs::remove_file(part)
        && part.symlink_metadata().is_ok()
    {
        log::warn!("can't remove {}: {error}", part.display());
    }
}

/// Brings `backup` back to `of` if nothing took that name. It is removed
/// only when the job's own replacement is known to hold the name, as the
/// user chose; when anything else does, it stays where it is, as the
/// only copy of the user's file.
fn restore(backup: &Path, of: &Path, replaced: bool) {
    if backup.symlink_metadata().is_err() {
        return;
    }
    let restored = if of.symlink_metadata().is_err() {
        rename_no_replace(backup, of)
    } else if replaced {
        fs::remove_file(backup)
    } else {
        log::warn!(
            "{} keeps the file that stood at {}, which something else holds now",
            backup.display(),
            of.display()
        );
        return;
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

#[cfg(test)]
mod tests;
