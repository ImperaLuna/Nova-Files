use nova_files_api::{JobId, Operation, Outcome, Tally};
use nova_files_open_api::{self as open_api, NotOpened};
use nova_files_trash_api as trash_api;
use nova_files_trash_ui::TrashPart;

use super::*;

#[gpui::test]
fn a_paused_job_says_so_until_it_resumes(cx: &mut TestAppContext) {
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    cx.simulate_keystrokes("down delete");
    let (tag, operation) = core
        .commands()
        .into_iter()
        .find_map(|command| match command {
            Command::StartJob {
                tag,
                operation: operation @ Operation::Trash { .. },
            } => Some((tag, operation)),
            _ => None,
        })
        .unwrap();
    let job = JobId(7);
    core.send(Event::JobStarted {
        tag,
        job,
        operation,
    });
    core.send(Event::JobPaused(job));
    cx.run_until_parked();
    let line = || view.read_with(cx, MainWindow::newest_job_line);
    assert!(line().is_some_and(|line| line.contains("paused")));

    core.send(Event::JobResumed(job));
    cx.run_until_parked();
    assert!(line().is_some_and(|line| !line.contains("paused")));
}

const TRASH: &str = "/trash/files";

fn trash_asks(core: &mut Core) -> usize {
    let mut asks = 0;
    while let Some(message) = core.try_recv() {
        if message.command::<trash_api::Command>() == Some(&trash_api::Command::List) {
            asks += 1;
        }
    }
    asks
}

#[gpui::test]
fn the_trash_is_read_again_on_arriving_in_it(cx: &mut TestAppContext) {
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    core.send(trash_api::Event::Listed {
        folder: Some(PathBuf::from(TRASH)),
        items: Vec::new(),
    });
    cx.run_until_parked();
    trash_asks(&mut core);

    view.update(cx, |view, _| view.open(Path::new(OTHER)));
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    assert_eq!(trash_asks(&mut core), 0);

    let part = view.read_with(cx, |view, _| view.part::<TrashPart>().unwrap());
    part.update(cx, |part, cx| part.open(cx));
    cx.run_until_parked();
    answer_listing(&mut core, 0);
    cx.run_until_parked();
    assert_eq!(trash_asks(&mut core), 1);

    let trash = view.read_with(cx, |view, _| view.folder().map(Path::to_path_buf));
    view.update(cx, |view, _| view.open(&trash.unwrap()));
    answer_listing(&mut core, 0);
    cx.run_until_parked();
    assert_eq!(trash_asks(&mut core), 0);
}

fn press(cx: &mut VisualTestContext, selector: &'static str) {
    let at = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_click(at, gpui::Modifiers::none());
}

fn start_copies(core: &mut Core) -> Vec<JobId> {
    let asked: Vec<_> = core
        .commands()
        .into_iter()
        .filter_map(|command| match command {
            Command::StartJob {
                tag,
                operation: operation @ Operation::Copy { .. },
            } => Some((tag, operation)),
            _ => None,
        })
        .collect();
    let mut jobs = Vec::new();
    for ((tag, operation), id) in asked.into_iter().zip(7..) {
        let job = JobId(id);
        core.send(Event::JobStarted {
            tag,
            job,
            operation,
        });
        jobs.push(job);
    }
    jobs
}

#[gpui::test]
fn several_copies_fold_into_one_line_that_opens_into_rows(cx: &mut TestAppContext) {
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    cx.simulate_keystrokes("down ctrl-c ctrl-v ctrl-v");
    cx.run_until_parked();
    let jobs = start_copies(&mut core);
    assert_eq!(jobs.len(), 2);
    cx.executor()
        .advance_clock(nova_files_window::JOB_SHOWS_AFTER);
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("island-progress").is_none(),
        "one line for both"
    );
    assert!(cx.debug_bounds("island-job-0").is_none(), "folded");
    assert!(
        cx.debug_bounds("island-jobs-bar").is_some(),
        "one bar for all"
    );

    press(cx, "island-jobs-open");
    assert!(
        cx.debug_bounds("island-jobs-bar").is_none(),
        "a bar per row"
    );
    assert!(cx.debug_bounds("island-job-0").is_some());
    assert!(cx.debug_bounds("island-job-1").is_some());
    press(cx, "island-stop-1");
    assert!(
        core.commands().contains(&Command::CancelJob(jobs[1])),
        "the newer one stops"
    );

    let finished = |job, outcome| Event::JobFinished {
        job,
        outcome,
        changed: Vec::new(),
        tally: Tally::default(),
    };
    let note = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.note().map(|note| note.label.to_string()))
    };
    let before = note(cx);
    core.send(finished(jobs[0], Outcome::Done));
    cx.run_until_parked();
    assert_eq!(note(cx), before);
    core.send(finished(jobs[1], Outcome::Cancelled));
    cx.run_until_parked();
    assert_eq!(note(cx).as_deref(), Some("Copied 1 item"));
}

#[gpui::test]
fn copies_all_cancelled_leave_no_note(cx: &mut TestAppContext) {
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    cx.simulate_keystrokes("down ctrl-c ctrl-v ctrl-v");
    cx.run_until_parked();
    let jobs = start_copies(&mut core);
    let note = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.note().map(|note| note.label.to_string()))
    };
    let before = note(cx);
    for job in jobs {
        core.send(Event::JobFinished {
            job,
            outcome: Outcome::Cancelled,
            changed: Vec::new(),
            tally: Tally::default(),
        });
        cx.run_until_parked();
        assert_eq!(note(cx), before, "{job:?}");
    }
}

#[gpui::test]
fn a_copy_shows_while_it_runs_and_one_that_replaced_reads_the_trash_again(cx: &mut TestAppContext) {
    cx.update(nova_files_window::bind_window_keys);
    let (_view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    cx.simulate_keystrokes("down");

    for (replaced, asks) in [(0, 0), (1, 1)] {
        cx.simulate_keystrokes("ctrl-c ctrl-v");
        cx.run_until_parked();
        let [job] = start_copies(&mut core)[..] else {
            panic!("one copy");
        };
        cx.executor()
            .advance_clock(nova_files_window::JOB_SHOWS_AFTER);
        cx.run_until_parked();
        assert!(cx.debug_bounds("island-progress").is_some());
        core.send(Event::JobFinished {
            job,
            outcome: Outcome::Done,
            changed: Vec::new(),
            tally: Tally {
                replaced,
                skipped: 0,
            },
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("island-progress").is_none());
        assert_eq!(trash_asks(&mut core), asks, "replaced {replaced}");
    }
}

#[gpui::test]
fn a_long_list_of_jobs_scrolls_instead_of_growing_past_the_window(cx: &mut TestAppContext) {
    cx.update(nova_files_window::bind_window_keys);
    let (_view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    cx.simulate_keystrokes("down ctrl-c");
    for _ in 0..12 {
        cx.simulate_keystrokes("ctrl-v");
    }
    cx.run_until_parked();
    assert_eq!(start_copies(&mut core).len(), 12);
    cx.executor()
        .advance_clock(nova_files_window::JOB_SHOWS_AFTER);
    cx.run_until_parked();

    press(cx, "island-jobs-open");
    let list = cx.debug_bounds("island-jobs").unwrap();
    let first = cx.debug_bounds("island-job-0").unwrap();
    assert!(
        list.size.height < first.size.height * 8.0,
        "{list:?} for rows of {first:?}"
    );
}

#[gpui::test]
fn files_whose_app_fails_say_so_and_the_rest_go_to_the_desktop(cx: &mut TestAppContext) {
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    cx.simulate_keystrokes("down shift-down enter");
    cx.run_until_parked();
    let opened: Vec<PathBuf> = core
        .sent::<open_api::Command>()
        .into_iter()
        .filter_map(|command| match command {
            open_api::Command::OpenFile { path, .. } => Some(path),
            open_api::Command::OpenWith(_) => None,
        })
        .collect();
    assert_eq!(opened.len(), 2, "{opened:?}");
    let note = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.note().map(|note| note.label.to_string()))
    };

    core.send(open_api::Event::NotOpened {
        path: opened[0].clone(),
        reason: NotOpened::NoApp,
    });
    cx.run_until_parked();
    assert_eq!(note(cx), None);
    assert_eq!(
        cx.opened_url(),
        Some(nova_files_api::uri::file_uri(&opened[0]))
    );
    core.send(open_api::Event::NotOpened {
        path: opened[1].clone(),
        reason: NotOpened::Sandboxed,
    });
    cx.run_until_parked();
    assert_eq!(
        cx.opened_url(),
        Some(nova_files_api::uri::file_uri(&opened[1]))
    );
    core.send(open_api::Event::NotOpened {
        path: opened[1].clone(),
        reason: NotOpened::Failed,
    });
    cx.run_until_parked();
    assert_eq!(
        note(cx).as_deref(),
        Some("Can't open 1: its app didn't start")
    );
    core.send(open_api::Event::NotOpened {
        path: opened[0].clone(),
        reason: NotOpened::Failed,
    });
    cx.run_until_parked();
    assert_eq!(
        note(cx).as_deref(),
        Some("Can't open 1, 0: their apps didn't start")
    );

    cx.executor().advance_clock(nova_files_window::NOTE_TIME);
    cx.run_until_parked();
    assert_eq!(note(cx), None);
    core.send(open_api::Event::NotOpened {
        path: opened[1].clone(),
        reason: NotOpened::Failed,
    });
    cx.run_until_parked();
    assert_eq!(
        note(cx).as_deref(),
        Some("Can't open 1: its app didn't start")
    );
}
