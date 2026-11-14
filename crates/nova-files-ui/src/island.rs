//! The island: one floating bar at the bottom of the sheet, where the
//! window says what it is doing and what the user can do next. It shows
//! one thing at a time, the most important first: a note after an action
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
use nova_files_api::{Command, Failure, JobId, Operation, Outcome};

/// How long a note stays up.
pub const NOTE_TIME: Duration = Duration::from_secs(6);

/// What the window has to say on its island, apart from the selection.
#[derive(Default)]
pub struct Island {
    pub note: Option<Note>,
    /// Takes the note down once [`NOTE_TIME`] has passed; replaced with
    /// each new note, which cancels the old one's.
    pub(crate) note_timer: Option<Task<()>>,
    /// Counts notes, so a timer only takes down the note it was set for.
    pub(crate) notes_shown: u64,
}

/// A line said after an action, maybe with a way to undo it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub label: SharedString,
    pub undo: Option<NoteUndo>,
    /// Which note this is (see [`Island::notes_shown`]).
    pub serial: u64,
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

/// "1 item", "3 items".
pub fn count(number: usize, one: &str, many: &str) -> String {
    if number == 1 {
        format!("1 {one}")
    } else {
        format!("{number} {many}")
    }
}

const ITEM: &str = "item";
const ITEMS: &str = "items";
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
fn what(paths: &[PathBuf]) -> String {
    match paths {
        [one] => name_of(one),
        many => count(many.len(), ITEM, ITEMS),
    }
}

/// The last part of `path` as a user reads it; the root is "/".
pub fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(|| ROOT.to_owned(), lossy)
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

/// The note once a job doing `operation` ended with `outcome`, while the
/// window shows `here`; `None` when there is nothing to say (a cancel, a
/// new folder, an undo).
pub fn finished_note(
    operation: &Operation,
    outcome: &Outcome,
    here: Option<&Path>,
) -> Option<String> {
    match outcome {
        Outcome::Done => done_note(operation, here),
        Outcome::Cancelled => None,
        Outcome::Failed { path, reason } => Some(match verb(operation) {
            (verb, true) => format!("Couldn't {verb} {}: {}", name_of(path), failure(*reason)),
            (verb, false) => format!("Couldn't {verb}: {}", failure(*reason)),
        }),
    }
}

fn done_note(operation: &Operation, here: Option<&Path>) -> Option<String> {
    let elsewhere = |into: &Path| {
        if here == Some(into) {
            String::new()
        } else {
            format!(" to {}", name_of(into))
        }
    };
    Some(match operation {
        Operation::Copy { sources, into } => format!("Copied {}{}", what(sources), elsewhere(into)),
        Operation::Move { sources, into } => {
            format!("Moved {} to {}", what(sources), name_of(into))
        }
        Operation::Trash { paths } => format!("Moved {} to {TRASH}", what(paths)),
        Operation::Restore { items } => format!("Restored {}", count(items.len(), ITEM, ITEMS)),
        Operation::Delete { paths } => format!("Deleted {}", what(paths)),
        Operation::DeleteFromTrash { items } => {
            format!("Deleted {}", count(items.len(), ITEM, ITEMS))
        }
        Operation::EmptyTrash => format!("{TRASH} emptied"),
        Operation::Rename { name, .. } => format!("Renamed to {}", lossy(name)),
        Operation::NewFolder { .. } | Operation::ChangeAccess { .. } | Operation::Undo { .. } => {
            return None;
        }
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

#[cfg(test)]
mod tests;
