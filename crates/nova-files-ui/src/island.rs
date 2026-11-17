//! The island: one floating bar at the bottom of the sheet, where the
//! window says what it is doing and what the user can do next. It shows
//! one thing at a time, the most important first: a running job's
//! progress (see `progress`), a question before a step that can't be
//! undone ("Delete 3 items for good?"), a note after an action
//! ("Moved 3 items to Trash", with Undo), then the selection ("3 selected
//! · 12 MB", with what can be done to it). With nothing to show it goes
//! down, and the sheet's quiet count ("4 folders, 9 files") shows in its
//! place.
//!
//! This module holds what the island knows and works out its words; the
//! window feeds it (see `window::acting` and `window::jobs`) and
//! `view::island` draws it. A note goes by itself after [`NOTE_TIME`].

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{SharedString, Task};
use nova_files_api::{Command, Entry, EntryKind, Failure, JobId, Operation, Outcome, Tally};

/// How long a note stays up.
pub const NOTE_TIME: Duration = Duration::from_secs(6);

/// What the window has to say on its island, apart from the selection.
#[derive(Default)]
pub struct Island {
    pub note: Option<Note>,
    /// A question before a step that can't be undone.
    pub confirm: Option<Confirm>,
    /// The conflict whose "For all" tick is ticked, by its job and its
    /// number there, so the tick never carries over to another question
    /// (see `asking`).
    pub for_all: Option<(JobId, u64)>,
    /// Takes the note down once [`NOTE_TIME`] has passed; replaced with
    /// each new note, which cancels the old one's.
    pub(crate) note_timer: Option<Task<()>>,
}

impl Island {
    /// Whether the "For all" tick is ticked on conflict `number` of `job`.
    pub fn ticked_for_all(&self, job: JobId, number: u64) -> bool {
        self.for_all == Some((job, number))
    }
}

/// A line said after an action, maybe with a way to undo it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub label: SharedString,
    pub undo: Option<NoteUndo>,
}

/// A question on the island before `operation`, which can't be undone:
/// `label`, with Cancel and `yes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub label: SharedString,
    pub yes: &'static str,
    pub operation: Operation,
}

/// What a note's Undo does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteUndo {
    /// Undo this job, which the core said can be undone.
    Job(JobId),
    /// Send these commands to the core: for a change that isn't a job,
    /// such as taking a folder out of Places.
    Send(Vec<Command>),
}

/// Takes the Undo off `note` when it undoes `job`, which can no longer
/// be undone. Whether it did.
pub fn drop_undo(note: &mut Option<Note>, job: JobId) -> bool {
    match note {
        Some(note) if note.undo == Some(NoteUndo::Job(job)) => {
            note.undo = None;
            true
        }
        _ => false,
    }
}

/// "1 item", "3 items".
pub fn count(number: usize, one: &str, many: &str) -> String {
    if number == 1 {
        format!("1 {one}")
    } else {
        format!("{number} {many}")
    }
}

pub const ITEM: &str = "item";
pub const ITEMS: &str = "items";
const FOLDER: &str = "folder";
const FOLDERS: &str = "folders";
const FILE: &str = "file";
const FILES: &str = "files";
const SELECTED: &str = "selected";
const PART_GAP: &str = " · ";
const LIST_GAP: &str = ", ";
const TRASH: &str = "Trash";
const ROOT: &str = "/";

/// A lone path by its name, several by how many.
pub fn what(paths: &[PathBuf]) -> String {
    match paths {
        [one] => name_of(one),
        many => count(many.len(), ITEM, ITEMS),
    }
}

/// The last part of `path` as a user reads it; the root is "/".
pub fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(|| ROOT.to_owned(), lossy)
}

/// `path` as a user reads it, with the home folder `home` as `~`.
pub fn short_path(path: &Path, home: &Path) -> String {
    use crate::pick::HOME;
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => HOME.to_owned(),
        Ok(rest) => format!("{HOME}/{}", rest.to_string_lossy()),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

fn lossy(name: &OsStr) -> String {
    name.to_string_lossy().into_owned()
}

/// What the selection line says: the count, drawn strong, and the rest.
pub fn selection_line(selected: usize, bytes: Option<u64>) -> (String, String) {
    let rest = bytes.filter(|&bytes| bytes > 0).map_or_else(
        || format!(" {SELECTED}"),
        |bytes| format!(" {SELECTED}{PART_GAP}{}", crate::columns::size(bytes)),
    );
    (selected.to_string(), rest)
}

/// The quiet count of `entries`: "4 folders, 9 files".
pub fn count_of(entries: &[Entry]) -> String {
    let folders = entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Folder)
        .count();
    quiet_count(folders, entries.len() - folders)
}

/// The quiet count under the list: "4 folders, 9 files"; empty for an
/// empty folder.
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

/// The note for files put on the clipboard: "Copied notes.txt", "Cut 3
/// items".
pub fn clipboard_note(cut: bool, paths: &[PathBuf]) -> String {
    let verb = if cut { "Cut" } else { "Copied" };
    format!("{verb} {}", what(paths))
}

/// The note once a job doing `operation` ended with `outcome`, while the
/// window shows `here`, with what `tally` says the answers to its
/// conflicts did; `None` when there is nothing to say (a cancel, a new
/// folder, an undo).
pub fn finished_note(
    operation: &Operation,
    outcome: &Outcome,
    here: Option<&Path>,
    tally: Tally,
) -> Option<String> {
    match outcome {
        Outcome::Done => match operation {
            Operation::Copy { sources, .. } | Operation::Move { sources, .. } => {
                answered_note(operation, sources, here, tally)
            }
            _ => done_note(operation),
        },
        Outcome::Cancelled => None,
        Outcome::Failed { path, reason } => Some(match verb(operation) {
            (verb, true) => format!("Couldn't {verb} {}: {}", name_of(path), failure(*reason)),
            (verb, false) => format!("Couldn't {verb}: {}", failure(*reason)),
        }),
    }
}

/// The note for a copy or move of `sources`, with what its answers
/// replaced or skipped: "Copied notes.txt", "Copied 4 items, replaced 2,
/// skipped 1", or "Nothing copied, skipped 3".
fn answered_note(
    operation: &Operation,
    sources: &[PathBuf],
    here: Option<&Path>,
    tally: Tally,
) -> Option<String> {
    let skipped = usize::try_from(tally.skipped).unwrap_or(usize::MAX);
    let went = sources.len().saturating_sub(skipped);
    let what = if skipped == 0 {
        what(sources)
    } else {
        count(went, ITEM, ITEMS)
    };
    let done = match operation {
        Operation::Copy { .. } if went == 0 => "Nothing copied".to_owned(),
        Operation::Move { .. } if went == 0 => "Nothing moved".to_owned(),
        Operation::Copy { into, .. } => format!("Copied {what}{}", elsewhere(into, here)),
        Operation::Move { into, .. } => format!("Moved {what} to {}", name_of(into)),
        _ => return None,
    };
    let mut parts = vec![done];
    if tally.replaced > 0 {
        parts.push(format!("replaced {}", tally.replaced));
    }
    if tally.skipped > 0 {
        parts.push(format!("skipped {}", tally.skipped));
    }
    Some(parts.join(LIST_GAP))
}

/// " to Documents" for a job into `into`, unless that is `here`, the
/// folder on screen.
fn elsewhere(into: &Path, here: Option<&Path>) -> String {
    if here == Some(into) {
        String::new()
    } else {
        format!(" to {}", name_of(into))
    }
}

fn done_note(operation: &Operation) -> Option<String> {
    Some(match operation {
        Operation::Trash { paths } => format!("Moved {} to {TRASH}", what(paths)),
        Operation::Restore { items } => format!("Restored {}", count(items.len(), ITEM, ITEMS)),
        Operation::Delete { paths } => format!("Deleted {}", what(paths)),
        Operation::DeleteFromTrash { items } => {
            format!("Deleted {}", count(items.len(), ITEM, ITEMS))
        }
        Operation::EmptyTrash => format!("{TRASH} emptied"),
        Operation::Rename { name, .. } => format!("Renamed to {}", lossy(name)),
        // Copies and moves say what their answers did (`answered_note`).
        Operation::Copy { .. }
        | Operation::Move { .. }
        | Operation::NewFolder { .. }
        | Operation::ChangeAccess { .. }
        | Operation::Undo { .. } => return None,
    })
}

/// What a job doing `operation` does, as the verb of a failure note, and
/// whether the path it failed at follows it.
const fn verb(operation: &Operation) -> (&'static str, bool) {
    match operation {
        Operation::Copy { .. } => ("copy", true),
        Operation::Move { .. } => ("move", true),
        Operation::Trash { .. } => ("move to Trash", true),
        Operation::Restore { .. } => ("restore", true),
        Operation::Delete { .. } | Operation::DeleteFromTrash { .. } => ("delete", true),
        Operation::Rename { .. } => ("rename", true),
        Operation::NewFolder { .. } => ("make", true),
        Operation::ChangeAccess { .. } => ("change", true),
        Operation::EmptyTrash => ("empty Trash", false),
        Operation::Undo { .. } => ("undo", false),
    }
}

/// Why a step failed, as a user reads it.
const fn failure(reason: Failure) -> &'static str {
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
        Failure::Other => "the system refused",
    }
}

pub mod asking;
pub mod progress;

#[cfg(test)]
mod tests;
