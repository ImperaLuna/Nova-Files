use std::any::Any;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use nova_files_api::{Operation, Tally};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Doing {
    pub verb: &'static str,
    pub after_items: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Failed {
    pub verb: &'static str,
    pub names_item: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grouped {
    pub one: &'static str,
    pub many: &'static str,
}

pub trait JobWords<J>: 'static {
    fn doing(&self, job: &J) -> Doing;

    fn sources<'a>(&self, job: &'a J) -> &'a [PathBuf];

    fn done(&self, job: &J, here: Option<&Path>, tally: Tally) -> Option<String>;

    fn failed(&self, job: &J) -> Failed;

    fn writes_into<'a>(&self, _: &'a J) -> Option<&'a Path> {
        None
    }

    fn grouped(&self) -> Option<Grouped> {
        None
    }

    fn title(&self, _: &J) -> Option<String> {
        None
    }
}

pub trait AnyJobWords {
    fn doing(&self, operation: &Operation) -> Option<Doing>;
    fn sources<'a>(&self, operation: &'a Operation) -> Option<&'a [PathBuf]>;
    fn done(
        &self,
        operation: &Operation,
        here: Option<&Path>,
        tally: Tally,
    ) -> Option<Option<String>>;
    fn failed(&self, operation: &Operation) -> Option<Failed>;
    fn writes_into<'a>(&self, operation: &'a Operation) -> Option<&'a Path>;
    fn grouped(&self, operation: &Operation) -> Option<Grouped>;
    fn title(&self, operation: &Operation) -> Option<String>;
}

struct Typed<J, W> {
    words: W,
    job: PhantomData<fn(&J)>,
}

impl<J: Any, W: JobWords<J>> AnyJobWords for Typed<J, W> {
    fn doing(&self, operation: &Operation) -> Option<Doing> {
        operation.get::<J>().map(|job| self.words.doing(job))
    }

    fn sources<'a>(&self, operation: &'a Operation) -> Option<&'a [PathBuf]> {
        operation.get::<J>().map(|job| self.words.sources(job))
    }

    fn done(
        &self,
        operation: &Operation,
        here: Option<&Path>,
        tally: Tally,
    ) -> Option<Option<String>> {
        operation
            .get::<J>()
            .map(|job| self.words.done(job, here, tally))
    }

    fn failed(&self, operation: &Operation) -> Option<Failed> {
        operation.get::<J>().map(|job| self.words.failed(job))
    }

    fn writes_into<'a>(&self, operation: &'a Operation) -> Option<&'a Path> {
        operation
            .get::<J>()
            .and_then(|job| self.words.writes_into(job))
    }

    fn grouped(&self, operation: &Operation) -> Option<Grouped> {
        operation.get::<J>().and_then(|_| self.words.grouped())
    }

    fn title(&self, operation: &Operation) -> Option<String> {
        operation.get::<J>().and_then(|job| self.words.title(job))
    }
}

#[derive(Clone, Default)]
pub struct Words(Vec<Rc<dyn AnyJobWords>>);

impl Words {
    pub fn add<J: Any, W: JobWords<J>>(&mut self, words: W) {
        self.0.push(Rc::new(Typed {
            words,
            job: PhantomData,
        }));
    }

    pub fn extend(&mut self, more: &Self) {
        self.0.extend(more.0.iter().cloned());
    }

    pub fn of<'a>(&'a self, operation: &Operation) -> Option<&'a dyn AnyJobWords> {
        self.0
            .iter()
            .find(|words| words.doing(operation).is_some())
            .map(AsRef::as_ref)
    }
}
