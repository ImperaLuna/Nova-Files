use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use gpui::{App, SharedString, Task};
use nova_files_api::JobId;
use nova_files_shell::{OnUndo, Undo};

pub const NOTE_TIME: Duration = Duration::from_secs(6);

#[derive(Default)]
pub struct Island {
    pub note: Option<Note>,
    pub confirm: Option<Confirm>,
    pub for_all: Option<(JobId, u64)>,
    pub jobs_open: bool,
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

pub use nova_files_shell::island::{asking, progress, several, words::*};

#[cfg(test)]
mod tests;
