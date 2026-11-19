use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gpui::{App, SharedString, Task};
use nova_files_api::{Entry, EntryKind, Failure, JobId, NotOpened, Operation, Outcome, Tally};
use nova_files_shell::{OnUndo, Undo};

pub const NOTE_TIME: Duration = Duration::from_secs(6);

#[derive(Default)]
pub struct Island {
    pub note: Option<Note>,
    pub confirm: Option<Confirm>,
    pub for_all: Option<(JobId, u64)>,
    pub jobs_open: bool,
    pub not_opened: Vec<(PathBuf, NotOpened)>,
    pub(crate) note_timer: Option<Task<()>>,
    pub(crate) note_owner: Option<nova_files_shell::NoteId>,
}

impl Island {
    pub fn ticked_for_all(&self, job: JobId, number: u64) -> bool {
        self.for_all == Some((job, number))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub label: SharedString,
    pub undo: Option<NoteUndo>,
}

pub struct Confirm {
    pub label: SharedString,
    pub yes: &'static str,
    pub on_yes: Box<dyn FnOnce(&mut App)>,
}

#[derive(Clone)]
pub enum NoteUndo {
    Job(JobId),
    Send(Vec<nova_files_gathered_api::Command>),
    Run(OnUndo),
}

impl From<Undo> for NoteUndo {
    fn from(undo: Undo) -> Self {
        match undo {
            Undo::Job(job) => Self::Job(job),
            Undo::Run(run) => Self::Run(run),
        }
    }
}

impl fmt::Debug for NoteUndo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Job(job) => f.debug_tuple("Job").field(job).finish(),
            Self::Send(commands) => f.debug_tuple("Send").field(commands).finish(),
            Self::Run(_) => f.debug_tuple("Run").finish_non_exhaustive(),
        }
    }
}

impl PartialEq for NoteUndo {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Job(one), Self::Job(other)) => one == other,
            (Self::Send(one), Self::Send(other)) => one == other,
            (Self::Run(one), Self::Run(other)) => Rc::ptr_eq(one, other),
            _ => false,
        }
    }
}

pub fn drop_undo(note: &mut Option<Note>, job: JobId) -> bool {
    match note {
        Some(note) if note.undo == Some(NoteUndo::Job(job)) => {
            note.undo = None;
            true
        }
        _ => false,
    }
}

pub use nova_files_shell::format::{ITEM, ITEMS, count};
const FOLDER: &str = "folder";
const FOLDERS: &str = "folders";
const FILE: &str = "file";
const FILES: &str = "files";
const SELECTED: &str = "selected";
const PART_GAP: &str = " · ";
const LIST_GAP: &str = ", ";
const TRASH: &str = "Trash";
const ROOT: &str = "/";

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

pub fn selection_line(selected: usize, bytes: Option<u64>) -> (String, String) {
    let rest = bytes.filter(|&bytes| bytes > 0).map_or_else(
        || format!(" {SELECTED}"),
        |bytes| format!(" {SELECTED}{PART_GAP}{}", crate::columns::size(bytes)),
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
) -> Option<String> {
    match outcome {
        Outcome::Done => match operation {
            Operation::Copy { sources, .. } | Operation::Move { sources, .. } => {
                answered_note(operation, sources, here, tally)
            }
            Operation::Extract { archive, .. } => Some(extracted_note(archive, tally)),
            _ => done_note(operation),
        },
        Outcome::Cancelled => None,
        Outcome::Failed { path, reason } => Some(match verb(operation) {
            (verb, true) => format!("Couldn't {verb} {}: {}", name_of(path), failure(*reason)),
            (verb, false) => format!("Couldn't {verb}: {}", failure(*reason)),
        }),
    }
}

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
    Some(with_answers(vec![done], tally))
}

fn extracted_note(archive: &Path, tally: Tally) -> String {
    with_answers(vec![format!("Extracted {}", name_of(archive))], tally)
}

fn with_answers(mut parts: Vec<String>, tally: Tally) -> String {
    if tally.replaced > 0 {
        parts.push(format!("replaced {}", tally.replaced));
    }
    if tally.skipped > 0 {
        parts.push(format!("skipped {}", tally.skipped));
    }
    parts.join(LIST_GAP)
}

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
        Operation::Compress { name, .. } => format!("Made {}", lossy(name)),
        Operation::Copy { .. }
        | Operation::Move { .. }
        | Operation::Extract { .. }
        | Operation::NewFolder { .. }
        | Operation::ChangeAccess { .. }
        | Operation::Undo { .. } => return None,
    })
}

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
        Operation::Extract { .. } => ("extract", true),
        Operation::Compress { .. } => ("compress", true),
    }
}

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
        Failure::Damaged => "the archive is damaged",
        Failure::NotArchive => "it isn't an archive Nova Files opens",
        Failure::Other => "the system refused",
    }
}

pub mod asking;
pub mod opening;
pub mod progress;
pub mod several;

#[cfg(test)]
mod tests;
