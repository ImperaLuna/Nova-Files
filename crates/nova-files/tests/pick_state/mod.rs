//! A picker's size between runs: however a picker closes, its size goes
//! into the windows' state, which the core writes to the state folder and hands
//! back to the next picker.

use std::time::{Duration, Instant};

use gpui::{Pixels, Size, px, size};
use nova_files_ui::{WindowState, fetch_window_state};

use super::*;

/// The state file under the tree's state folder, as `picker_with` sets it.
fn state_file(tree: &TempFolder) -> PathBuf {
    tree.path().join(".local/state/nova-files/window-state")
}

/// Waits until the core has written `expected` to the state file.
fn wait_for_state(tree: &TempFolder, expected: &WindowState) {
    let start = Instant::now();
    let want = expected.to_bytes();
    while fs::read(state_file(tree)).ok() != Some(want.clone()) {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the state file never held {:?}",
            String::from_utf8_lossy(&want)
        );
        std::thread::yield_now();
    }
}

fn window_size(cx: &mut VisualTestContext) -> Size<Pixels> {
    cx.update(|window, _| window.window_bounds().get_bounds().size)
}

#[gpui::test]
async fn cancelling_saves_the_size(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (_view, cx, _answers) = picker(cx, &tree, tree.path(), FILES).await;
    cx.simulate_resize(size(px(700.0), px(500.0)));
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    wait_for_state(
        &tree,
        &WindowState::default().with_picker(size(px(700.0), px(500.0))),
    );
}

#[gpui::test]
async fn picking_saves_the_size(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (_view, cx, answers) = picker(cx, &tree, tree.path(), FILES).await;
    let opened = window_size(cx);
    click(cx, "entry-2");
    cx.simulate_keystrokes("enter");
    assert!(answers.try_recv().is_ok());
    wait_for_state(&tree, &WindowState::default().with_picker(opened));
}

#[gpui::test]
async fn closing_the_window_saves_the_size(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FILES).await;
    let opened = window_size(cx);
    cx.update(|window, _| window.remove_window());
    // The test's handle is the last one keeping the view.
    drop(view);
    // Freed entities are dropped at the end of the app's next update.
    let app: &mut TestAppContext = cx;
    app.update(|_| {});
    assert_eq!(answers.try_recv().ok(), Some(PickAnswer::Cancelled));
    wait_for_state(&tree, &WindowState::default().with_picker(opened));
}

#[test]
fn the_next_picker_gets_the_state_the_core_kept() {
    let tree = TempFolder::tree("pick-state");
    let home = nova_files_core::Home {
        folder: tree.path().to_owned(),
        config: tree.path().join(".config"),
        state: tree.path().join(".local/state"),
        data: tree.path().join(".local/share"),
    };
    let saved = WindowState::default().with_picker(size(px(800.0), px(500.0)));
    let core =
        nova_files_core::spawn_with(nova_files_core::IDLE_EXIT, Some(home.clone()), None, None)
            .unwrap();
    assert_eq!(fetch_window_state(&core.connector), WindowState::default());
    let (sink, _events) = mpsc::channel();
    let client = core.connector.connect(sink).unwrap();
    client
        .send(nova_files_api::Command::SaveWindowState(saved.to_bytes()))
        .unwrap();
    assert_eq!(fetch_window_state(&core.connector), saved);
    // A core started later reads it from the state folder.
    wait_for_state(&tree, &saved);
    let next =
        nova_files_core::spawn_with(nova_files_core::IDLE_EXIT, Some(home), None, None).unwrap();
    assert_eq!(fetch_window_state(&next.connector), saved);
}
