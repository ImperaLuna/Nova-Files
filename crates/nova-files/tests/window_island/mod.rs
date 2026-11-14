//! The island and what it acts on, on a real core and a temp folder
//! tree: the quiet count while nothing is selected, the selection's
//! line and buttons, moving to the trash by key or button, Undo, and a
//! note going by itself.

use super::*;

/// A window on `folder`, with the window's keys bound as the program
/// binds them, and a home of its own in `home`, so the trash and the
/// core's state stay out of the folder on screen.
async fn island_window<'a>(
    cx: &'a mut TestAppContext,
    folder: &Path,
    home: &TempFolder,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    cx.update(nova_files_ui::bind_window_keys);
    let (view, cx) = window_on(cx, folder, home.path());
    wait_for_folder(&view, cx, folder).await;
    cx.run_until_parked();
    (view, cx)
}

/// Waits until the folder on screen lists exactly `names`.
async fn wait_for_names(view: &Entity<MainWindow>, cx: &VisualTestContext, names: &[&str]) {
    view.condition::<WindowEvent>(cx, |view, _| view.entry_names() == names)
        .await;
}

/// Waits until the island's note reads `label`.
async fn wait_for_note(view: &Entity<MainWindow>, cx: &VisualTestContext, label: &str) {
    view.condition::<WindowEvent>(cx, |view, _| {
        view.note().is_some_and(|note| note.label == label)
    })
    .await;
}

#[gpui::test]
async fn the_count_shows_until_something_is_selected(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("island");
    let home = TempFolder::tree("island-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    assert!(cx.debug_bounds("count").is_some());
    assert!(cx.debug_bounds("island").is_none());
    click(cx, "name-1");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.selected_count()), 1);
    assert!(cx.debug_bounds("island").is_some());
    assert!(cx.debug_bounds("count").is_none());

    click(cx, "island-clear");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.selected_count()), 0);
    assert!(cx.debug_bounds("island").is_none());
}

#[gpui::test]
async fn delete_moves_to_the_trash_and_undo_puts_it_back(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("island");
    let home = TempFolder::tree("island-home");
    let notes = tree.path().join("notes.txt");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-1");
    cx.simulate_keystrokes("delete");
    wait_for_names(&view, cx, &["Projects"]).await;
    assert!(!notes.exists());
    wait_for_note(&view, cx, "Moved notes.txt to Trash").await;
    let trashed = home.path().join(".local/share/Trash/files/notes.txt");
    assert!(trashed.exists());

    click(cx, "island-undo");
    wait_for_names(&view, cx, &["Projects", "notes.txt"]).await;
    assert!(notes.exists());
    assert!(!trashed.exists());
}

#[gpui::test]
async fn the_island_trash_button_and_ctrl_z_do_the_same(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("island");
    let home = TempFolder::tree("island-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-0");
    click(cx, "island-trash");
    wait_for_names(&view, cx, &["notes.txt"]).await;
    assert!(!tree.path().join("Projects").exists());
    wait_for_note(&view, cx, "Moved Projects to Trash").await;

    // With the note gone, Ctrl+Z still undoes the window's last job.
    click(cx, "island-dismiss");
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.note().is_none()));
    cx.simulate_keystrokes("ctrl-z");
    wait_for_names(&view, cx, &["Projects", "notes.txt"]).await;
    assert!(tree.path().join("Projects/nova").is_dir());
}

#[gpui::test]
async fn a_note_goes_by_itself(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("island");
    let home = TempFolder::tree("island-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-1");
    cx.simulate_keystrokes("delete");
    wait_for_note(&view, cx, "Moved notes.txt to Trash").await;
    cx.executor().advance_clock(nova_files_ui::NOTE_TIME);
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.note().is_none()));
    assert!(cx.debug_bounds("count").is_some());
}
