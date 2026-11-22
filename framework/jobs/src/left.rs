use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use nova_files_api::{JobId, Operation};

use super::journal::read::{Line, read};
use super::journal::{BACKUP_PREFIX, PART_PREFIX, entry_from, hold, word};
use super::place::rename_no_replace;
use super::plan::Entry;
use super::sync::remove;
use crate::trash_info;

#[derive(Debug)]
pub struct Left {
    pub job: JobId,
    pub operation: Operation,
    pub done: Done,
    pub journal: PathBuf,
    pub file: File,
    pub whole: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Done {
    pub made: HashMap<PathBuf, PathBuf>,
    pub planned: Option<HashMap<PathBuf, Entry>>,
    pub trashed: HashSet<PathBuf>,
    pub leaving: HashMap<PathBuf, Entry>,
}

pub type Rebuild<'a> = &'a dyn Fn(&str, Option<PathBuf>, Vec<PathBuf>) -> Option<Operation>;

pub fn recover(folder: &Path, rebuild: Rebuild<'_>) -> Vec<Left> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut left: Vec<_> = entries
        .flatten()
        .filter_map(|entry| look_at(entry.path(), entry.file_name().to_str(), rebuild))
        .collect();
    left.sort_by_key(|left| left.job);
    left
}

fn look_at(journal: PathBuf, name: Option<&str>, rebuild: Rebuild<'_>) -> Option<Left> {
    let file = open(&journal)
        .inspect_err(|error| log::info!("{} is left alone: {error}", journal.display()))
        .ok()?;
    let written = read(&file, &journal)
        .inspect_err(|error| log::warn!("can't read {}: {error}", journal.display()))
        .ok()?;
    if foreign(&written.lines, rebuild) {
        log::info!(
            "{} is left alone: no feature here knows its job",
            journal.display()
        );
        return None;
    }
    let done = clean(&written.lines);
    let job = name.and_then(|name| name.parse().ok());
    let (Some(job), Some(operation)) = (job, operation(&written.lines, rebuild)) else {
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

fn open(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new().read(true).append(true).open(path)?;
    hold(&file)?;
    Ok(file)
}

fn operation(lines: &[Line], rebuild: Rebuild<'_>) -> Option<Operation> {
    if !lines.iter().any(|line| line.word == word::SOURCES_DONE) {
        return None;
    }
    let sources: Vec<_> = first_paths(lines, word::SOURCE).collect();
    let into = first_paths(lines, word::INTO).next();
    rebuild(lines.first()?.word.as_str(), into, sources)
}

fn foreign(lines: &[Line], rebuild: Rebuild<'_>) -> bool {
    lines.first().is_some_and(|first| {
        word::fields(&first.word).is_none()
            && rebuild(
                &first.word,
                first_paths(lines, word::INTO).next(),
                first_paths(lines, word::SOURCE).collect(),
            )
            .is_none()
    })
}

fn first_paths<'a>(lines: &'a [Line], word: &'a str) -> impl Iterator<Item = PathBuf> + 'a {
    lines
        .iter()
        .filter(move |line| line.word == word)
        .filter_map(|line| line.paths.first().cloned())
}

fn clean(lines: &[Line]) -> Done {
    let mut done = Done::default();
    let mut placed = HashSet::new();
    let mut planned = HashMap::new();
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
            (word::TRASHED, [from, _]) => {
                done.trashed.insert(from.clone());
            }
            (word::TRASH_INFO, [info, _, item]) => {
                if let (Some(stored), Some(item)) =
                    (trash_info::item_of_info(info), entry_from(item))
                {
                    done.leaving.insert(stored, item);
                }
            }
            (word::PLANNED, [path, entry]) => {
                if let Some(entry) = entry_from(entry) {
                    planned.insert(path.clone(), entry);
                }
            }
            (word::PLAN_DONE, []) => whole_plan = true,
            _ => {}
        }
    }
    if whole_plan {
        done.planned = Some(planned);
    }
    let trashing = lines.first().is_some_and(|line| line.word == word::TRASH);
    let putting_back = lines
        .iter()
        .position(|line| line.word == word::PUT_BACK)
        .unwrap_or(lines.len());
    let trash_lines = &lines[..putting_back];
    let last_info = trash_lines
        .iter()
        .rposition(|line| line.word == word::TRASH_INFO);
    let settled = |at: usize| {
        trashing
            && at < putting_back
            && (Some(at) != last_info
                || trash_lines[at..]
                    .iter()
                    .any(|line| line.word == word::TRASHED))
    };
    for (at, line) in lines.iter().enumerate() {
        match (line.word.as_str(), line.paths.as_slice()) {
            (word::PART, [part]) if hidden(part, PART_PREFIX) => remove(part),
            (word::BACKUP, [backup, of]) if hidden(backup, BACKUP_PREFIX) => {
                restore(backup, of, placed.contains(of));
            }
            (word::TRASH_INFO, [info, entry, item]) if !settled(at) => {
                if let (Some(entry), Some(item)) = (entry_from(entry), entry_from(item)) {
                    remove_lone_info(info, entry, item);
                }
            }
            _ => {}
        }
    }
    done
}

pub fn remove_lone_info(info: &Path, entry: Entry, item: Entry) {
    let Some(stored) = trash_info::item_of_info(info) else {
        return;
    };
    let ours = info
        .symlink_metadata()
        .is_ok_and(|there| entry.is(&Entry::of(&there)));
    if ours
        && match stored.symlink_metadata() {
            Ok(there) => !item.is(&Entry::of(&there)),
            Err(error) => error.kind() == io::ErrorKind::NotFound,
        }
    {
        remove(info);
    }
}

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

fn hidden(path: &Path, prefix: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(prefix))
}

#[cfg(test)]
mod tests;
