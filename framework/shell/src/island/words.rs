use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use nova_files_api::{Entry, EntryKind, Failure, Operation, Outcome, Tally};

use super::job_words::{AnyJobWords, Words};

pub use crate::format::{ITEM, ITEMS, count};
const FOLDER: &str = "folder";
const FOLDERS: &str = "folders";
const FILE: &str = "file";
const FILES: &str = "files";
const SELECTED: &str = "selected";
pub const PART_GAP: &str = " · ";
const LIST_GAP: &str = ", ";
const ROOT: &str = "/";
const FINISH: &str = "finish";

pub fn what(paths: &[PathBuf]) -> String {
    match paths {
        [one] => name_of(one),
        many => count(many.len(), ITEM, ITEMS),
    }
}

pub fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(|| ROOT.to_owned(), lossy)
}

pub fn short_path(path: &Path, home: &Path) -> String {
    use crate::typed::HOME;
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => HOME.to_owned(),
        Ok(rest) => format!("{HOME}/{}", rest.to_string_lossy()),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

fn lossy(name: &OsStr) -> String {
    name.to_string_lossy().into_owned()
}

pub fn selection_line(selected: usize, bytes: Option<u64>) -> (String, String) {
    let rest = bytes.filter(|&bytes| bytes > 0).map_or_else(
        || format!(" {SELECTED}"),
        |bytes| format!(" {SELECTED}{PART_GAP}{}", crate::format::size(bytes)),
    );
    (selected.to_string(), rest)
}

pub fn count_of(entries: &[Entry]) -> String {
    let folders = entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Folder)
        .count();
    quiet_count(folders, entries.len() - folders)
}

pub fn quiet_count(folders: usize, files: usize) -> String {
    let mut parts = Vec::new();
    if folders > 0 {
        parts.push(count(folders, FOLDER, FOLDERS));
    }
    if files > 0 {
        parts.push(count(files, FILE, FILES));
    }
    parts.join(LIST_GAP)
}

pub fn finished_note(
    operation: &Operation,
    outcome: &Outcome,
    here: Option<&Path>,
    tally: Tally,
    words: &Words,
) -> Option<String> {
    let feature = words.of(operation);
    match outcome {
        Outcome::Done if matches!(operation, Operation::Feature(_)) => {
            feature.and_then(|words| words.done(operation, here, tally).flatten())
        }
        Outcome::Done => done_note(operation),
        Outcome::Cancelled => None,
        Outcome::Failed { path, reason } => {
            let why = feature
                .and_then(|words| words.why(operation, *reason))
                .unwrap_or_else(|| failure_in(*reason));
            Some(match verb(operation, feature) {
                (verb, true) => format!("Couldn't {verb} {}: {why}", name_of(path)),
                (verb, false) => format!("Couldn't {verb}: {why}"),
            })
        }
    }
}

pub fn with_answers(mut parts: Vec<String>, tally: Tally) -> String {
    if tally.replaced > 0 {
        parts.push(format!("replaced {}", tally.replaced));
    }
    if tally.skipped > 0 {
        parts.push(format!("skipped {}", tally.skipped));
    }
    parts.join(LIST_GAP)
}

fn done_note(operation: &Operation) -> Option<String> {
    Some(match operation {
        Operation::Rename { name, .. } => format!("Renamed to {}", lossy(name)),
        Operation::NewFolder { .. }
        | Operation::ChangeAccess { .. }
        | Operation::Undo { .. }
        | Operation::Feature(_) => return None,
    })
}

fn verb(operation: &Operation, words: Option<&dyn AnyJobWords>) -> (&'static str, bool) {
    match operation {
        Operation::Feature(_) => words
            .and_then(|words| words.failed(operation))
            .map_or((FINISH, false), |failed| (failed.verb, failed.names_item)),
        Operation::Rename { .. } => ("rename", true),
        Operation::NewFolder { .. } => ("make", true),
        Operation::ChangeAccess { .. } => ("change", true),
        Operation::Undo { .. } => ("undo", false),
    }
}

const fn failure_in(reason: Failure) -> &'static str {
    match reason {
        Failure::Unsupported => "Nova Files can't do that with it",
        _ => failure(reason),
    }
}

pub const fn failure(reason: Failure) -> &'static str {
    match reason {
        Failure::NameTaken => "the name is taken",
        Failure::InvalidName => "that name can't be used",
        Failure::NotFound => "it isn't there any more",
        Failure::PermissionDenied => "permission denied",
        Failure::NoSpace => "the drive is full",
        Failure::ReadOnly => "the drive is read-only",
        Failure::IntoItself => "a folder can't go inside itself",
        Failure::Unsupported => "it can't be copied",
        Failure::SourceChanged => "it changed while being copied",
        Failure::NoRecord => "Trash has no record of where it came from",
        Failure::Damaged => "the archive is damaged",
        Failure::NotArchive => "it isn't an archive Nova Files opens",
        Failure::Other => "the system refused",
    }
}

#[cfg(test)]
#[path = "words_tests.rs"]
mod tests;
