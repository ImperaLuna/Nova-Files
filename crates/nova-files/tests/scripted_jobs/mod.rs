//! Jobs and the trash against a scripted core: the island follows what
//! the core says about a job, arriving in the trash reads it again, and
//! files that open in no app say so.

use nova_files_api::{JobId, NotOpened, Operation};

use super::*;

#[gpui::test]
fn a_paused_job_says_so_until_it_resumes(cx: &mut TestAppContext) {
    cx.update(nova_files_ui::bind_window_keys);
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

/// How many times the window asked the core for the trash since last
/// asked.
fn trash_asks(core: &mut Core) -> usize {
    core.commands()
        .into_iter()
        .filter(|command| matches!(command, Command::ListTrash))
        .count()
}

#[gpui::test]
fn the_trash_is_read_again_on_arriving_in_it(cx: &mut TestAppContext) {
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    trash_asks(&mut core);

    view.update(cx, |view, _| view.open(Path::new(OTHER)));
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    assert_eq!(trash_asks(&mut core), 0);

    view.update(cx, MainWindow::open_trash);
    answer_listing(&mut core, 0);
    cx.run_until_parked();
    assert_eq!(trash_asks(&mut core), 1);

    // Reading it again where it is asks nothing more.
    let trash = view.read_with(cx, |view, _| view.folder().map(Path::to_path_buf));
    view.update(cx, |view, _| view.open(&trash.unwrap()));
    answer_listing(&mut core, 0);
    cx.run_until_parked();
    assert_eq!(trash_asks(&mut core), 0);
}

/// Clicks the element drawn with `selector`.
fn press(cx: &mut VisualTestContext, selector: &'static str) {
    let at = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_click(at, gpui::Modifiers::none());
}

/// The copies the window asked the core for since last asked, each
/// started as a job, the first as 7.
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
    cx.update(nova_files_ui::bind_window_keys);
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    cx.simulate_keystrokes("down ctrl-c ctrl-v ctrl-v");
    cx.run_until_parked();
    let jobs = start_copies(&mut core);
    assert_eq!(jobs.len(), 2);
    cx.executor().advance_clock(nova_files_ui::JOB_SHOWS_AFTER);
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("island-progress").is_none(),
        "one line for both"
    );
    assert!(cx.debug_bounds("island-job-0").is_none(), "folded");

    press(cx, "island-jobs-open");
    assert!(cx.debug_bounds("island-job-0").is_some());
    assert!(cx.debug_bounds("island-job-1").is_some());
    press(cx, "island-stop-1");
    assert!(
        core.commands().contains(&Command::CancelJob(jobs[1])),
        "the newer one stops"
    );

    // The first ends while the other runs: no note yet; one when the last
    // ends.
    let finished = |job, outcome| Event::JobFinished {
        job,
        outcome,
        changed: Vec::new(),
        tally: nova_files_api::Tally::default(),
    };
    let note = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.note().map(|note| note.label.to_string()))
    };
    // What Ctrl+C said.
    let before = note(cx);
    core.send(finished(jobs[0], nova_files_api::Outcome::Done));
    cx.run_until_parked();
    assert_eq!(note(cx), before);
    core.send(finished(jobs[1], nova_files_api::Outcome::Cancelled));
    cx.run_until_parked();
    assert_eq!(note(cx).as_deref(), Some("Copied 1 item"));
}

#[gpui::test]
fn files_that_open_in_no_app_each_say_why_on_the_island(cx: &mut TestAppContext) {
    cx.update(nova_files_ui::bind_window_keys);
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    cx.simulate_keystrokes("down shift-down enter");
    cx.run_until_parked();
    let opened: Vec<PathBuf> = core
        .commands()
        .into_iter()
        .filter_map(|command| match command {
            Command::OpenFile { path, .. } => Some(path),
            _ => None,
        })
        .collect();
    assert_eq!(opened.len(), 2, "{opened:?}");
    let note = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.note().map(|note| note.label.to_string()))
    };

    core.send(Event::FileNotOpened {
        path: opened[0].clone(),
        reason: NotOpened::NoApp,
    });
    cx.run_until_parked();
    assert_eq!(note(cx).as_deref(), Some("Can't open 0: no app opens it"));
    core.send(Event::FileNotOpened {
        path: opened[1].clone(),
        reason: NotOpened::Failed,
    });
    cx.run_until_parked();
    assert_eq!(
        note(cx).as_deref(),
        Some("Can't open 0: no app opens it · 1: its app didn't start")
    );
    // The same file failing again is named once, with its latest reason.
    core.send(Event::FileNotOpened {
        path: opened[0].clone(),
        reason: NotOpened::Failed,
    });
    cx.run_until_parked();
    assert_eq!(
        note(cx).as_deref(),
        Some("Can't open 1, 0: their apps didn't start")
    );

    // Gone by itself; a later failure starts its own note.
    cx.executor().advance_clock(nova_files_ui::NOTE_TIME);
    cx.run_until_parked();
    assert_eq!(note(cx), None);
    core.send(Event::FileNotOpened {
        path: opened[1].clone(),
        reason: NotOpened::Failed,
    });
    cx.run_until_parked();
    assert_eq!(
        note(cx).as_deref(),
        Some("Can't open 1: its app didn't start")
    );
}
