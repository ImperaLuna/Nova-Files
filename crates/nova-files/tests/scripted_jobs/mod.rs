//! A job against a scripted core: the island follows what the core says
//! about it.

use nova_files_api::{JobId, Operation};

use super::*;

#[gpui::test]
fn a_paused_job_says_so_until_it_resumes(cx: &mut TestAppContext) {
    cx.update(nova_files_ui::bind_window_keys);
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    cx.simulate_keystrokes("down delete");
    let tag = core
        .commands()
        .into_iter()
        .find_map(|command| match command {
            Command::StartJob {
                tag,
                operation: Operation::Trash { .. },
            } => Some(tag),
            _ => None,
        })
        .unwrap();
    let job = JobId(7);
    core.send(Event::JobStarted { tag, job });
    core.send(Event::JobPaused(job));
    cx.run_until_parked();
    let line = || view.read_with(cx, MainWindow::newest_job_line);
    assert!(line().is_some_and(|line| line.contains("paused")));

    core.send(Event::JobResumed(job));
    cx.run_until_parked();
    assert!(line().is_some_and(|line| !line.contains("paused")));
}
