use std::path::{Path, PathBuf};

use nova_files_api::{Answer, ConflictKind, Failure, Tally};
use nova_files_shell::island::job_words::{Batched, Doing, Failed, Grouped, JobWords};
use nova_files_shell::island::words::{ITEM, ITEMS, count, failure, name_of, what, with_answers};
use nova_files_transfer_api::{CopyJob, LinkJob, MoveJob};

struct Said {
    doing: &'static str,
    verb: &'static str,
    batched: Batched,
    grouped: Grouped,
}

const COPY: Said = Said {
    doing: "Copying",
    verb: "copy",
    batched: Batched {
        done: "Copied",
        done_after: "copied",
        nothing: "Nothing copied",
    },
    grouped: Grouped {
        one: "copy",
        many: "copies",
    },
};

const MOVE: Said = Said {
    doing: "Moving",
    verb: "move",
    batched: Batched {
        done: "Moved",
        done_after: "moved",
        nothing: "Nothing moved",
    },
    grouped: Grouped {
        one: "move",
        many: "moves",
    },
};

const LINK: Said = Said {
    doing: "Linking",
    verb: "link",
    batched: Batched {
        done: "Linked",
        done_after: "linked",
        nothing: "Nothing linked",
    },
    grouped: Grouped {
        one: "link",
        many: "links",
    },
};

fn done_note(said: &Said, sources: &[PathBuf], where_to: &str, tally: Tally) -> String {
    let skipped = usize::try_from(tally.skipped).unwrap_or(usize::MAX);
    let went = sources.len().saturating_sub(skipped);
    let done = if went == 0 {
        said.batched.nothing.to_owned()
    } else if skipped == 0 {
        format!("{} {}{where_to}", said.batched.done, what(sources))
    } else {
        format!(
            "{} {}{where_to}",
            said.batched.done,
            count(went, ITEM, ITEMS)
        )
    };
    with_answers(vec![done], tally)
}

fn to(into: &Path) -> String {
    format!(" to {}", name_of(into))
}

fn title(sources: &[PathBuf], into: &Path) -> String {
    format!("{}{}", what(sources), to(into))
}

const fn doing(said: &Said) -> Doing {
    Doing {
        verb: said.doing,
        after_items: "",
    }
}

const fn failed(said: &Said) -> Failed {
    Failed {
        verb: said.verb,
        names_item: true,
    }
}

pub struct LinkWords;

impl JobWords<LinkJob> for LinkWords {
    fn doing(&self, _: &LinkJob) -> Doing {
        doing(&LINK)
    }

    fn sources<'a>(&self, job: &'a LinkJob) -> &'a [PathBuf] {
        &job.sources
    }

    fn done(&self, job: &LinkJob, here: Option<&Path>, tally: Tally) -> Option<String> {
        let where_to = if here == Some(job.into.as_path()) {
            String::new()
        } else {
            format!(" in {}", name_of(&job.into))
        };
        Some(done_note(&LINK, &job.sources, &where_to, tally))
    }

    fn failed(&self, _: &LinkJob) -> Failed {
        failed(&LINK)
    }

    fn writes_into<'a>(&self, job: &'a LinkJob) -> Option<&'a Path> {
        Some(&job.into)
    }
}

pub struct CopyWords;

impl JobWords<CopyJob> for CopyWords {
    fn doing(&self, _: &CopyJob) -> Doing {
        doing(&COPY)
    }

    fn sources<'a>(&self, job: &'a CopyJob) -> &'a [PathBuf] {
        &job.sources
    }

    fn done(&self, job: &CopyJob, here: Option<&Path>, tally: Tally) -> Option<String> {
        let where_to = if here == Some(job.into.as_path()) {
            String::new()
        } else {
            to(&job.into)
        };
        Some(done_note(&COPY, &job.sources, &where_to, tally))
    }

    fn failed(&self, _: &CopyJob) -> Failed {
        failed(&COPY)
    }

    fn writes_into<'a>(&self, job: &'a CopyJob) -> Option<&'a Path> {
        Some(&job.into)
    }

    fn grouped(&self) -> Option<Grouped> {
        Some(COPY.grouped)
    }

    fn title(&self, job: &CopyJob) -> Option<String> {
        Some(title(&job.sources, &job.into))
    }

    fn batched(&self) -> Option<Batched> {
        Some(COPY.batched)
    }

    fn answers_itself(&self, _: &CopyJob, kind: &ConflictKind) -> Option<Answer> {
        (*kind == ConflictKind::SameFile).then_some(Answer::KeepBoth)
    }

    fn why(&self, _: &CopyJob, reason: Failure) -> Option<&'static str> {
        Some(failure(reason))
    }
}

pub struct MoveWords;

impl JobWords<MoveJob> for MoveWords {
    fn doing(&self, _: &MoveJob) -> Doing {
        doing(&MOVE)
    }

    fn sources<'a>(&self, job: &'a MoveJob) -> &'a [PathBuf] {
        &job.sources
    }

    fn done(&self, job: &MoveJob, _: Option<&Path>, tally: Tally) -> Option<String> {
        Some(done_note(&MOVE, &job.sources, &to(&job.into), tally))
    }

    fn failed(&self, _: &MoveJob) -> Failed {
        failed(&MOVE)
    }

    fn writes_into<'a>(&self, job: &'a MoveJob) -> Option<&'a Path> {
        Some(&job.into)
    }

    fn grouped(&self) -> Option<Grouped> {
        Some(MOVE.grouped)
    }

    fn title(&self, job: &MoveJob) -> Option<String> {
        Some(title(&job.sources, &job.into))
    }

    fn batched(&self) -> Option<Batched> {
        Some(MOVE.batched)
    }

    fn why(&self, _: &MoveJob, reason: Failure) -> Option<&'static str> {
        Some(failure(reason))
    }
}

#[cfg(test)]
#[path = "words_tests.rs"]
mod tests;
