//! Trash as a place: Was in, Restore, Delete for good and Empty Trash,
//! on a real core with a home of its own.

use super::*;

/// Moves `name` in the folder on screen to the trash, then opens the
/// trash and waits for it to list `listed`.
async fn trash_and_open(
    view: &Entity<MainWindow>,
    cx: &mut VisualTestContext,
    selector: &'static str,
    note: &str,
    listed: &[&str],
) {
    click(cx, selector);
    cx.simulate_keystrokes("delete");
    wait_for_note(view, cx, note).await;
    view.update(cx, MainWindow::open_trash);
    wait_for_names(view, cx, listed).await;
    assert!(view.read_with(cx, |view, _| view.trash_open()));
}

#[gpui::test]
async fn the_trash_says_where_each_came_from_and_restores_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("trash-place");
    let home = TempFolder::tree("trash-place-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    trash_and_open(
        &view,
        cx,
        "name-1",
        "Moved notes.txt to Trash",
        &["notes.txt"],
    )
    .await;
    let was_in = tree.path().to_string_lossy().into_owned();
    view.condition::<WindowEvent>(cx, |view, _| {
        view.was_in(0)
            .is_some_and(|folder| folder == was_in.as_str())
    })
    .await;
    assert_eq!(view.read_with(cx, |view, _| view.trash_count()), Some(1));
    cx.run_until_parked();
    assert!(cx.debug_bounds("head-was-in").is_some());
    assert!(cx.debug_bounds("empty-trash").is_some());

    click(cx, "island-dismiss");
    click(cx, "name-0");
    cx.run_until_parked();
    // In the trash, the selection can only go back.
    assert!(cx.debug_bounds("island-copy").is_none());
    click(cx, "island-restore");
    wait_for_names(&view, cx, &[]).await;
    wait_for_note(&view, cx, "Restored 1 item").await;
    assert!(tree.path().join("notes.txt").is_file());
    view.condition::<WindowEvent>(cx, |view, _| view.trash_count() == Some(0))
        .await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("trash-empty").is_some());
    assert!(cx.debug_bounds("empty-trash").is_none());
}

#[gpui::test]
async fn empty_trash_asks_first(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("trash-empty");
    let home = TempFolder::tree("trash-empty-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    trash_and_open(
        &view,
        cx,
        "name-0",
        "Moved Projects to Trash",
        &["Projects"],
    )
    .await;
    click(cx, "empty-trash");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view
            .confirm()
            .map(|confirm| confirm.label.clone())),
        Some("Delete 1 item in Trash for good?".into())
    );
    click(cx, "island-no");
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.confirm().is_none()));
    assert_eq!(
        view.read_with(cx, |view, _| view.entry_names()),
        ["Projects"]
    );

    click(cx, "empty-trash");
    cx.run_until_parked();
    click(cx, "island-yes");
    wait_for_names(&view, cx, &[]).await;
    wait_for_note(&view, cx, "Trash emptied").await;
    assert!(!tree.path().join("Projects").exists());
}

#[gpui::test]
async fn delete_in_the_trash_asks_then_deletes_for_good(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("trash-delete");
    let home = TempFolder::tree("trash-delete-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    trash_and_open(
        &view,
        cx,
        "name-1",
        "Moved notes.txt to Trash",
        &["notes.txt"],
    )
    .await;
    click(cx, "name-0");
    cx.simulate_keystrokes("delete");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view
            .confirm()
            .map(|confirm| confirm.label.clone())),
        Some("Delete 1 item for good?".into())
    );
    click(cx, "island-yes");
    wait_for_names(&view, cx, &[]).await;
    assert!(!tree.path().join("notes.txt").exists());
}

#[gpui::test]
async fn a_trash_never_used_shows_empty(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("trash-new");
    let home = TempFolder::tree("trash-new-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    view.update(cx, MainWindow::open_trash);
    view.condition::<WindowEvent>(cx, |view, _| view.trash_open())
        .await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("trash-empty").is_some());
    assert!(cx.debug_bounds("empty-trash").is_none());
    // Back goes to where the window was.
    assert_eq!(
        view.read_with(cx, |view, _| view.back_target().map(Path::to_path_buf)),
        Some(tree.path().to_path_buf())
    );
}
