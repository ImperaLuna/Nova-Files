use std::fmt;

use gpui::{App, Context, EventEmitter, SharedString};
use nova_files_api::{JobId, Operation};

pub type OnYes = Box<dyn FnOnce(&mut App)>;

pub enum IslandAsk {
    StartJob(Operation),
    Note {
        label: SharedString,
        undo: Option<JobId>,
    },
    Ask {
        question: SharedString,
        yes: &'static str,
        on_yes: OnYes,
    },
}

impl fmt::Debug for IslandAsk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StartJob(operation) => f.debug_tuple("StartJob").field(operation).finish(),
            Self::Note { label, undo } => f
                .debug_struct("Note")
                .field("label", label)
                .field("undo", undo)
                .finish(),
            Self::Ask { question, yes, .. } => f
                .debug_struct("Ask")
                .field("question", question)
                .field("yes", yes)
                .finish_non_exhaustive(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IslandAsked;

#[derive(Debug, Default)]
pub struct Island {
    asks: Vec<IslandAsk>,
}

impl EventEmitter<IslandAsked> for Island {}

impl Island {
    pub fn note(
        &mut self,
        label: impl Into<SharedString>,
        undo: Option<JobId>,
        cx: &mut Context<Self>,
    ) {
        self.asks.push(IslandAsk::Note {
            label: label.into(),
            undo,
        });
        cx.emit(IslandAsked);
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

    pub fn take_asks(&mut self) -> Vec<IslandAsk> {
        std::mem::take(&mut self.asks)
    }
}
