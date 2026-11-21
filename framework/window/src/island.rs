use std::time::Duration;

use gpui::{App, Entity, SharedString, Subscription, Task};
use nova_files_api::JobId;
use nova_files_shell::Undo;
use nova_files_shell::text::TextField;

pub const NOTE_TIME: Duration = Duration::from_secs(6);

#[derive(Default)]
pub struct Island {
    pub note: Option<Note>,
    pub confirm: Option<Confirm>,
    pub for_all: Option<(JobId, u64)>,
    pub jobs_open: bool,
    pub(crate) note_timer: Option<Task<()>>,
    pub(crate) note_owner: Option<nova_files_shell::NoteId>,
    pub(crate) password: Option<Password>,
}

pub struct Password {
    pub(crate) job: JobId,
    pub(crate) number: u64,
    pub(crate) field: Entity<TextField>,
    pub(crate) _events: Subscription,
}

impl Island {
    pub fn ticked_for_all(&self, job: JobId, number: u64) -> bool {
        self.for_all == Some((job, number))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub label: SharedString,
    pub undo: Option<Undo>,
}

pub struct Confirm {
    pub label: SharedString,
    pub yes: &'static str,
    pub on_yes: Box<dyn FnOnce(&mut App)>,
}

pub fn drop_undo(note: &mut Option<Note>, job: JobId) -> bool {
    match note {
        Some(note) if note.undo == Some(Undo::Job(job)) => {
            note.undo = None;
            true
        }
        _ => false,
    }
}

pub use nova_files_shell::island::{asking, progress, several, words::*};

#[cfg(test)]
mod tests;
