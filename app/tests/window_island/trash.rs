use super::*;

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
    open_trash(view, cx, false).await;
    wait_for_names(view, cx, listed).await;
    assert!(view.read_with(cx, trash_open));
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
    view.condition::<WindowEvent>(cx, |view, cx| {
        trash_was_in(view, 0, cx).is_some_and(|folder| folder == was_in.as_str())
    })
    .await;
    assert_eq!(view.read_with(cx, trash_count), Some(1));
    cx.run_until_parked();
    assert!(cx.debug_bounds("head-was-in").is_some());
    assert!(cx.debug_bounds("place-trash-here").is_some());
    assert!(cx.debug_bounds("place-trash-number").is_some());
    open_trash(&view, cx, true).await;
    view.read_with(cx, |view, _| {
        assert_eq!(view.tab_names(), ["Trash", "Trash"]);
    });
    click(cx, "tab-0");
    cx.run_until_parked();
    let chip = cx.debug_bounds("empty-trash").unwrap();
    let slot = cx.debug_bounds("path").unwrap();
    assert!(slot.right() - chip.right() < chip.left() - slot.left());
    assert!(slot.right() - chip.right() < px(10.0));

    click(cx, "island-dismiss");
    click(cx, "name-0");
    cx.run_until_parked();
    assert!(cx.debug_bounds("island-copy").is_none());
    click(cx, "island-restore");
    wait_for_names(&view, cx, &[]).await;
    wait_for_note(&view, cx, "Restored 1 item").await;
    assert!(tree.path().join("notes.txt").is_file());
    view.condition::<WindowEvent>(cx, |view, cx| trash_count(view, cx) == Some(0))
        .await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("empty-note").is_some());
    assert!(cx.debug_bounds("empty-trash").is_none());
    assert!(cx.debug_bounds("place-trash-number").is_none());
}

#[gpui::test]
async fn empty_trash_asks_first(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("empty-note");
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
    delete_in_the_trash(cx, "delete").await;
}

#[gpui::test]
async fn shift_delete_in_the_trash_does_the_same(cx: &mut TestAppContext) {
    delete_in_the_trash(cx, "shift-delete").await;
}

async fn delete_in_the_trash(cx: &mut TestAppContext, key: &str) {
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
    cx.simulate_keystrokes(key);
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

    open_trash(&view, cx, false).await;
    view.condition::<WindowEvent>(cx, trash_open).await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("empty-note").is_some());
    assert!(cx.debug_bounds("empty-trash").is_none());
    assert_eq!(
        view.read_with(cx, |view, _| view.back_target().cloned()),
        Some(Location::from(tree.path()))
    );
}

#[gpui::test]
async fn in_the_trash_files_only_go_back(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("trash-only-back");
    let home = TempFolder::tree("trash-only-back-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-0");
    cx.simulate_keystrokes("ctrl-c");
    trash_and_open(
        &view,
        cx,
        "name-1",
        "Moved notes.txt to Trash",
        &["notes.txt"],
    )
    .await;
    click(cx, "island-dismiss");
    click(cx, "name-0");
    cx.simulate_keystrokes("ctrl-c");
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.note().is_none()));
    cx.simulate_keystrokes("ctrl-v");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.jobs_pending()), 0);
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |view, _| view.renaming()));
    assert_eq!(
        view.read_with(cx, |view, _| view.entry_names()),
        ["notes.txt"]
    );
}

#[gpui::test]
async fn undoing_elsewhere_stays_in_the_folder_on_screen(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("trash-stay");
    let home = TempFolder::tree("trash-stay-home");
    let notes = tree.path().join("notes.txt");
    let projects = tree.path().join("Projects");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-1");
    cx.simulate_keystrokes("delete");
    wait_for_note(&view, cx, "Moved notes.txt to Trash").await;
    view.update(cx, |view, _| view.open(&projects));
    wait_for_names(&view, cx, &["nova"]).await;
    cx.simulate_keystrokes("ctrl-z");
    view.condition::<WindowEvent>(cx, |view, _| view.jobs_pending() == 0)
        .await;
    cx.run_until_parked();
    assert!(notes.is_file());
    assert!(!view.read_with(cx, trash_open));
    assert_eq!(view.read_with(cx, |view, _| view.entry_names()), ["nova"]);
}

#[gpui::test]
async fn an_empty_folder_is_not_called_an_empty_trash(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("trash-not");
    let home = TempFolder::tree("trash-not-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    view.update(cx, |view, _| view.open(&tree.path().join("Projects/nova")));
    wait_for_names(&view, cx, &[]).await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("empty-note").is_none());
}

#[gpui::test]
async fn the_path_bar_in_the_trash_is_one_crumb(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("trash-crumb");
    let home = TempFolder::tree("trash-crumb-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    trash_and_open(
        &view,
        cx,
        "name-0",
        "Moved Projects to Trash",
        &["Projects"],
    )
    .await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("crumb-0").is_some());
    assert!(cx.debug_bounds("crumb-1").is_none());
    assert!(cx.debug_bounds("arrow-0").is_none());
}
