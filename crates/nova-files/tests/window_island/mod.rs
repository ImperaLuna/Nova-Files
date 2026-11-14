//! The island and what it acts on, on a real core and a temp folder
//! tree: the quiet count while nothing is selected, the selection's
//! line and buttons, copy, cut and paste through the clipboard, moving
//! to the trash by key or button, Undo, and a note going by itself.
//! Renaming and New folder are in `renaming`.

use std::fs;

mod renaming;
mod trash;

use super::*;

/// A window on `folder`, with the window's and text fields' keys bound
/// as the program binds them, and a home of its own in `home`, so the trash and the
/// core's state stay out of the folder on screen.
async fn island_window<'a>(
    cx: &'a mut TestAppContext,
    folder: &Path,
    home: &TempFolder,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        nova_files_ui::bind_text_keys(cx);
        nova_files_ui::bind_window_keys(cx);
    });
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
async fn copy_and_paste_copies_into_the_folder_on_screen(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("island");
    let home = TempFolder::tree("island-home");
    fs::write(tree.path().join("notes.txt"), "kept").unwrap();
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-1");
    click(cx, "island-copy");
    wait_for_note(&view, cx, "Copied notes.txt").await;
    double_click(cx, "entry-0");
    let projects = tree.path().join("Projects");
    wait_for_folder(&view, cx, &projects).await;

    click(cx, "island-dismiss");
    cx.simulate_keystrokes("ctrl-v");
    wait_for_names(&view, cx, &["nova", "notes.txt"]).await;
    assert_eq!(
        fs::read_to_string(projects.join("notes.txt")).unwrap(),
        "kept"
    );
    assert!(tree.path().join("notes.txt").exists());
    wait_for_note(&view, cx, "Copied notes.txt").await;
}

#[gpui::test]
async fn cut_and_paste_moves_and_undo_moves_back(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("island");
    let home = TempFolder::tree("island-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-1");
    cx.simulate_keystrokes("ctrl-x");
    wait_for_note(&view, cx, "Cut notes.txt").await;
    // Pasting where the files already are does nothing.
    cx.simulate_keystrokes("ctrl-v");
    cx.run_until_parked();
    double_click(cx, "entry-0");
    let projects = tree.path().join("Projects");
    wait_for_folder(&view, cx, &projects).await;

    click(cx, "island-dismiss");
    cx.simulate_keystrokes("ctrl-v");
    wait_for_names(&view, cx, &["nova", "notes.txt"]).await;
    assert!(!tree.path().join("notes.txt").exists());
    wait_for_note(&view, cx, "Moved notes.txt to Projects").await;
    // The cut pastes once.
    assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()), None);

    click(cx, "island-undo");
    wait_for_names(&view, cx, &["nova"]).await;
    assert!(tree.path().join("notes.txt").exists());
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

#[gpui::test]
async fn a_job_held_at_a_conflict_shows_waiting_and_cancels(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("island");
    let home = TempFolder::tree("island-home");
    let projects = tree.path().join("Projects");
    fs::write(projects.join("notes.txt"), "there first").unwrap();
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-1");
    cx.simulate_keystrokes("ctrl-c");
    double_click(cx, "entry-0");
    wait_for_folder(&view, cx, &projects).await;
    cx.simulate_keystrokes("ctrl-v");
    view.condition::<WindowEvent>(cx, |view, cx| {
        view.newest_job_line(cx).as_deref() == Some("Copying notes.txt · waiting")
    })
    .await;
    // A quick job only leaves its note; this one holds, so it shows.
    cx.run_until_parked();
    assert!(cx.debug_bounds("island-progress").is_none());
    cx.executor().advance_clock(nova_files_ui::JOB_SHOWS_AFTER);
    cx.run_until_parked();
    assert!(cx.debug_bounds("island-progress").is_some());

    click(cx, "island-cancel");
    view.condition::<WindowEvent>(cx, |view, cx| view.newest_job_line(cx).is_none())
        .await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("island-progress").is_none());
    assert_eq!(
        fs::read_to_string(projects.join("notes.txt")).unwrap(),
        "there first"
    );
}

#[gpui::test]
async fn pasting_a_copy_where_it_came_from_keeps_both(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("island");
    let home = TempFolder::tree("island-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-1");
    cx.simulate_keystrokes("ctrl-c");
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-v");
    wait_for_names(&view, cx, &["Projects", "notes (2).txt", "notes.txt"]).await;
}
