use nova_files_preview_ui::PreviewPart;

use super::*;

fn previewing(view: &Entity<MainWindow>, cx: &VisualTestContext) -> bool {
    view.read_with(cx, |view, cx| {
        view.part::<PreviewPart>()
            .is_some_and(|part| part.read(cx).previewing())
    })
}

#[gpui::test]
async fn keys_while_previewing_change_nothing_behind_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview-keys");
    let home = TempFolder::tree("preview-keys-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-1");
    cx.simulate_keystrokes("space");
    cx.run_until_parked();
    assert!(previewing(&view, cx));
    cx.simulate_keystrokes("delete shift-delete f2 ctrl-z ctrl-d");
    cx.run_until_parked();
    assert!(tree.path().join("notes.txt").exists());
    assert!(!tree.path().join("notes (2).txt").exists());
    assert!(!view.read_with(cx, MainWindow::renaming));
    assert!(previewing(&view, cx));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!previewing(&view, cx));
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_names()),
        ["notes.txt"]
    );

    cx.simulate_keystrokes("space");
    cx.run_until_parked();
    assert!(previewing(&view, cx));
    cx.simulate_keystrokes("space");
    cx.run_until_parked();
    assert!(!previewing(&view, cx));
}
