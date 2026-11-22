use std::path::Path;

use gpui::{Context, InteractiveElement as _, IntoElement as _, Styled as _, Window, div};
use nova_files_api::{Answer, Conflict, ConflictKind, JobId, Operation, Outcome, Question, Tally};
use nova_files_shell::island::job_words::{Doing, Failed, JobWords};
use nova_files_shell::{Asking, Host, Part, Parts, Slots};

use super::super::*;

#[derive(Debug, PartialEq, Eq)]
struct Pack {
    files: Vec<PathBuf>,
}

struct PackWords;

impl JobWords<Pack> for PackWords {
    fn doing(&self, _: &Pack) -> Doing {
        Doing {
            verb: "Packing",
            after_items: "",
        }
    }

    fn sources<'a>(&self, job: &'a Pack) -> &'a [PathBuf] {
        &job.files
    }

    fn done(&self, job: &Pack, _: Option<&Path>, _: Tally) -> Option<String> {
        Some(format!("Packed {}", job.files.len()))
    }

    fn failed(&self, _: &Pack) -> Failed {
        Failed {
            verb: "pack",
            names_item: true,
        }
    }
}

struct Packer {
    asked: Option<(JobId, u64)>,
}

static SEALED: Question = Question::new("sealed", &[Answer::Retry]);

impl Packer {
    fn asks(&mut self, asking: Option<Asking<'_>>, _: &mut Window, _: &mut Context<Self>) {
        self.asked = asking.map(|asking| (asking.job, asking.conflict.number));
    }
}

struct Loud;

impl Part for Loud {
    fn open(_: &Host, _: &mut Context<Self>) -> Self {
        Self
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.island(
            |_, _, _| true,
            |_, _, _, _| {
                Some(
                    div()
                        .debug_selector(|| "loud-island".to_owned())
                        .size_4()
                        .into_any_element(),
                )
            },
        );
    }
}

impl Part for Packer {
    fn open(_: &Host, _: &mut Context<Self>) -> Self {
        Self { asked: None }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.job_words::<Pack>(PackWords);
        slots.question(&SEALED, Self::asks, |_, _, _, _| {
            Some(
                div()
                    .debug_selector(|| "pack-question".to_owned())
                    .size_4()
                    .into_any_element(),
            )
        });
    }
}

fn asked(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Option<(JobId, u64)> {
    let packer = view.read_with(cx, |view, _| view.part::<Packer>().unwrap());
    packer.read_with(cx, |packer, _| packer.asked)
}

fn packing(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    let mut parts = Parts::default();
    parts.add::<Loud>();
    parts.add::<Packer>();
    cx.update(|cx| cx.set_global(parts));
    let (view, cx, mut core) = window_with_parts(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    (view, cx, core)
}

fn start_pack(view: &Entity<MainWindow>, cx: &mut VisualTestContext, core: &mut Core) -> JobId {
    let island = view.read_with(cx, |view, _| view.host().island.clone());
    let pack = Operation::feature(Pack {
        files: vec![PathBuf::from("/a/notes.txt")],
    });
    island.update(cx, |island, cx| island.start_job(pack, cx));
    cx.run_until_parked();
    let (tag, operation) = core
        .commands()
        .into_iter()
        .find_map(|command| match command {
            Command::StartJob { tag, operation } => Some((tag, operation)),
            _ => None,
        })
        .unwrap();
    let job = JobId(3);
    core.send(Event::JobStarted {
        tag,
        job,
        operation,
    });
    cx.run_until_parked();
    job
}

#[gpui::test]
fn a_feature_job_is_told_in_its_own_words(cx: &mut TestAppContext) {
    let (view, cx, mut core) = packing(cx);
    let job = start_pack(&view, cx, &mut core);

    assert_eq!(
        view.read_with(cx, MainWindow::newest_job_line).as_deref(),
        Some("Packing notes.txt")
    );

    core.send(Event::JobFinished {
        job,
        outcome: Outcome::Done,
        changed: Vec::new(),
        tally: Tally::default(),
    });
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.note().map(|note| note.label.to_string())),
        Some("Packed 1".to_owned())
    );
}

#[gpui::test]
fn a_feature_question_is_drawn_by_its_feature(cx: &mut TestAppContext) {
    let (view, cx, mut core) = packing(cx);
    let job = start_pack(&view, cx, &mut core);
    assert!(cx.debug_bounds("pack-question").is_none());

    core.send(Event::JobConflict {
        job,
        conflict: Conflict {
            kind: ConflictKind::Asked(&SEALED),
            source: PathBuf::from("/a/notes.txt"),
            target: PathBuf::new(),
            number: 1,
            more: 0,
        },
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("pack-question").is_some());
    assert!(cx.debug_bounds("loud-island").is_none());
    assert_eq!(asked(&view, &*cx), Some((job, 1)));

    core.send(Event::ConflictAnswered {
        job,
        answer: Answer::Retry,
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("pack-question").is_none());
    assert_eq!(asked(&view, &*cx), None);
}
