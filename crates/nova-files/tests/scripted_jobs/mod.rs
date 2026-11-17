//! Jobs and the trash against a scripted core: the island follows what
//! the core says about a job, and arriving in the trash reads it again.

use nova_files_api::{JobId, Operation, Outcome, Tally};

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

/// Pastes the entry selected as a copy, starts it as job `job` on the
/// scripted core and lets it run long enough to show on the island.
fn run_a_copy(core: &mut Core, cx: &mut VisualTestContext, job: JobId) {
    cx.simulate_keystrokes("ctrl-c ctrl-v");
    cx.run_until_parked();
    let (tag, operation) = core
        .commands()
        .into_iter()
        .find_map(|command| match command {
            Command::StartJob {
                tag,
                operation: operation @ Operation::Copy { .. },
            } => Some((tag, operation)),
            _ => None,
        })
        .unwrap();
    core.send(Event::JobStarted {
        tag,
        job,
        operation,
    });
    cx.run_until_parked();
    cx.executor().advance_clock(nova_files_ui::JOB_SHOWS_AFTER);
    cx.run_until_parked();
}

#[gpui::test]
fn a_copy_shows_while_it_runs_and_one_that_replaced_reads_the_trash_again(cx: &mut TestAppContext) {
    cx.update(nova_files_ui::bind_window_keys);
    let (_view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    cx.simulate_keystrokes("down");

    for (job, replaced, asks) in [(JobId(3), 0, 0), (JobId(4), 1, 1)] {
        run_a_copy(&mut core, cx, job);
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
