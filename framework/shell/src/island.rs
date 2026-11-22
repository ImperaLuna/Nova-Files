pub mod asking;
pub mod job_words;
pub mod look;
pub mod progress;
pub mod several;
pub mod words;

#[cfg(test)]
#[path = "island/tests/jobs.rs"]
mod test_jobs;

use std::fmt;
use std::rc::Rc;

use gpui::{App, Context, EventEmitter, SharedString};
use nova_files_api::{JobId, Operation};

pub type OnYes = Box<dyn FnOnce(&mut App)>;
pub type OnUndo = Rc<dyn Fn(&mut App)>;

#[derive(Clone)]
pub enum Undo {
    Job(JobId),
    Run(OnUndo),
}

impl fmt::Debug for Undo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Job(job) => f.debug_tuple("Job").field(job).finish(),
            Self::Run(_) => f.debug_tuple("Run").finish_non_exhaustive(),
        }
    }
}

impl PartialEq for Undo {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Job(one), Self::Job(other)) => one == other,
            (Self::Run(one), Self::Run(other)) => Rc::ptr_eq(one, other),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteGone(pub NoteId);

pub enum IslandAsk {
    StartJob(Operation),
    Note {
        id: NoteId,
        label: SharedString,
        undo: Option<Undo>,
    },
    Ask {
        question: SharedString,
        yes: &'static str,
        on_yes: OnYes,
    },
    DismissNote,
}

impl fmt::Debug for IslandAsk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StartJob(operation) => f.debug_tuple("StartJob").field(operation).finish(),
            Self::Note { id, label, undo } => f
                .debug_struct("Note")
                .field("id", id)
                .field("label", label)
                .field("undo", undo)
                .finish(),
            Self::Ask { question, yes, .. } => f
                .debug_struct("Ask")
                .field("question", question)
                .field("yes", yes)
                .finish_non_exhaustive(),
            Self::DismissNote => f.write_str("DismissNote"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IslandAsked;

#[derive(Debug, Default)]
pub struct Island {
    asks: Vec<IslandAsk>,
    notes: u64,
    undoable: Option<Undo>,
    busy: bool,
}

impl EventEmitter<IslandAsked> for Island {}
impl EventEmitter<NoteGone> for Island {}

impl Island {
    pub fn note(
        &mut self,
        label: impl Into<SharedString>,
        undo: Option<Undo>,
        cx: &mut Context<Self>,
    ) -> NoteId {
        self.notes += 1;
        let id = NoteId(self.notes);
        self.asks.push(IslandAsk::Note {
            id,
            label: label.into(),
            undo,
        });
        cx.emit(IslandAsked);
        id
    }

    pub fn gone(&mut self, id: NoteId, cx: &mut Context<Self>) {
        cx.emit(NoteGone(id));
    }

    pub fn ask(
        &mut self,
        question: impl Into<SharedString>,
        yes: &'static str,
        on_yes: OnYes,
        cx: &mut Context<Self>,
    ) {
        self.asks.push(IslandAsk::Ask {
            question: question.into(),
            yes,
            on_yes,
        });
        cx.emit(IslandAsked);
    }

    pub fn start_job(&mut self, operation: Operation, cx: &mut Context<Self>) {
        self.asks.push(IslandAsk::StartJob(operation));
        cx.emit(IslandAsked);
    }

    pub fn set_undoable(&mut self, undo: Option<Undo>, busy: bool) {
        self.undoable = undo;
        self.busy = busy;
    }

    pub const fn busy(&self) -> bool {
        self.busy
    }

    pub fn take_undo(&mut self, cx: &mut Context<Self>) -> Option<Undo> {
        if self.busy {
            return None;
        }
        let undo = self.undoable.take();
        self.asks.push(IslandAsk::DismissNote);
        cx.emit(IslandAsked);
        undo
    }

    pub fn take_asks(&mut self) -> Vec<IslandAsk> {
        std::mem::take(&mut self.asks)
    }
}
