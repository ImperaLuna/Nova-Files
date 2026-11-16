//! The island and what it acts on, on a real core and a temp folder
//! tree: the quiet count while nothing is selected, the selection's
//! line and buttons, copy, cut and paste through the clipboard, moving
//! to the trash by key or button, Undo, and a note going by itself.
//! Renaming and New folder are in `renaming`, the trash in `trash`, and
//! the right-click menus in `menus`.

use std::fs;

mod menus;
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
async fn a_clash_is_asked_on_the_island_and_undo_puts_the_replaced_back(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("island");
    let home = TempFolder::tree("island-home");
    let projects = tree.path().join("Projects");
    fs::write(tree.path().join("notes.txt"), "new").unwrap();
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
    // Asked at once, with what a file clash takes; one clash, no tick.
    cx.run_until_parked();
    for answer in ["island-skip", "island-keep-both", "island-replace"] {
        assert!(cx.debug_bounds(answer).is_some(), "{answer}");
    }
    assert!(cx.debug_bounds("island-merge").is_none());
    assert!(cx.debug_bounds("island-for-all").is_none());
    assert!(cx.debug_bounds("island-progress").is_none());

    click(cx, "island-replace");
    wait_for_note(&view, cx, "Copied notes.txt, replaced 1").await;
    assert_eq!(
        fs::read_to_string(projects.join("notes.txt")).unwrap(),
        "new"
    );

    click(cx, "island-undo");
    view.condition::<WindowEvent>(cx, |_, _| {
        fs::read_to_string(projects.join("notes.txt")).is_ok_and(|text| text == "there first")
    })
    .await;
}

#[gpui::test]
async fn for_all_answers_every_clash_like_it_at_once(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("island");
    let home = TempFolder::tree("island-home");
    let projects = tree.path().join("Projects");
    for name in ["a.txt", "b.txt", "notes.txt"] {
        fs::write(projects.join(name), "there first").unwrap();
    }
    fs::write(tree.path().join("a.txt"), "new").unwrap();
    fs::write(tree.path().join("b.txt"), "new").unwrap();
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-1");
    click_with(cx, "name-3", Modifiers::shift());
    cx.simulate_keystrokes("ctrl-c");
    double_click(cx, "entry-0");
    wait_for_folder(&view, cx, &projects).await;
    cx.simulate_keystrokes("ctrl-v");
    view.condition::<WindowEvent>(cx, |view, cx| {
        view.newest_job_line(cx)
            .is_some_and(|line| line.ends_with("· waiting"))
    })
    .await;
    cx.run_until_parked();

    click(cx, "island-for-all");
    click(cx, "island-skip");
    wait_for_note(&view, cx, "Nothing copied, skipped 3").await;
    for name in ["a.txt", "b.txt", "notes.txt"] {
        assert_eq!(
            fs::read_to_string(projects.join(name)).unwrap(),
            "there first"
        );
    }
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

#[gpui::test]
async fn the_selection_line_counts_bytes_of_files_only(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("island-bytes");
    let home = TempFolder::tree("island-bytes-home");
    fs::write(tree.path().join("notes.txt"), "hello world").unwrap();
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-1");
    view.condition::<WindowEvent>(cx, |view, _| view.selected_bytes() == Some(11))
        .await;
    // A folder selected too adds nothing.
    cx.simulate_keystrokes("ctrl-a");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_bytes()),
        Some(11)
    );
}

#[gpui::test]
async fn delete_with_nothing_selected_does_nothing(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("island-none");
    let home = TempFolder::tree("island-none-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    cx.simulate_keystrokes("delete");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.jobs_pending()), 0);
}

#[gpui::test]
async fn a_note_stays_its_full_time_after_another_goes(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("island-notes");
    let home = TempFolder::tree("island-notes-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    view.update(cx, |view, cx| view.show_note("first", None, cx));
    cx.executor().advance_clock(nova_files_ui::NOTE_TIME / 2);
    view.update(cx, |view, cx| view.show_note("second", None, cx));
    // The first note's time is up; the second's isn't.
    cx.executor()
        .advance_clock(nova_files_ui::NOTE_TIME / 2 + nova_files_ui::NOTE_TIME / 4);
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.note().map(|note| note.label.clone())),
        Some("second".into())
    );
    cx.executor().advance_clock(nova_files_ui::NOTE_TIME / 2);
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.note().is_none()));
}

#[gpui::test]
async fn two_jobs_asked_at_once_are_both_pending(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("island-two");
    let home = TempFolder::tree("island-two-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-0");
    view.update(cx, |view, cx| {
        view.trash_selected(cx);
        view.trash_selected(cx);
        assert_eq!(view.jobs_pending(), 2);
    });
}

#[gpui::test]
async fn an_undone_job_is_not_undone_again(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("island-again");
    let home = TempFolder::tree("island-again-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-1");
    cx.simulate_keystrokes("delete");
    wait_for_note(&view, cx, "Moved notes.txt to Trash").await;
    cx.simulate_keystrokes("ctrl-z");
    wait_for_names(&view, cx, &["Projects", "notes.txt"]).await;
    view.condition::<WindowEvent>(cx, |view, _| view.jobs_pending() == 0)
        .await;
    // The core no longer offers it: another Ctrl+Z asks for nothing,
    // rather than failing to undo it again.
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.note().is_none()));
}
