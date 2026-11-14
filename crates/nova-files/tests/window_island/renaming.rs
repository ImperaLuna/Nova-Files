//! Rename in place and New folder, on a real core.

use super::*;

/// The text and selection of the rename field on row `index`.
fn field_state(
    view: &Entity<MainWindow>,
    cx: &VisualTestContext,
    index: usize,
) -> (String, std::ops::Range<usize>) {
    view.read_with(cx, |view, cx| {
        let field = view.rename_field(index).unwrap().read(cx);
        (field.text().to_owned(), field.selection())
    })
}

#[gpui::test]
async fn f2_renames_in_place_and_undo_names_it_back(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("rename");
    let home = TempFolder::tree("rename-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-1");
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    // The name is selected up to its extension.
    assert_eq!(field_state(&view, cx, 1), ("notes.txt".to_owned(), 0..5));
    cx.simulate_input("plans");
    cx.simulate_keystrokes("enter");
    wait_for_names(&view, cx, &["Projects", "plans.txt"]).await;
    assert!(tree.path().join("plans.txt").exists());
    assert!(!view.read_with(cx, |view, _| view.renaming()));
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_names()),
        ["plans.txt"]
    );
    wait_for_note(&view, cx, "Renamed to plans.txt").await;

    click(cx, "island-undo");
    wait_for_names(&view, cx, &["Projects", "notes.txt"]).await;
}

#[gpui::test]
async fn escape_or_the_same_name_leaves_it_as_it_was(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("rename");
    let home = TempFolder::tree("rename-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-0");
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    // A folder's whole name is selected.
    assert_eq!(field_state(&view, cx, 0), ("Projects".to_owned(), 0..8));
    cx.simulate_input("Elsewhere");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |view, _| view.renaming()));
    // Escape took the field away, not the selection.
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_names()),
        ["Projects"]
    );

    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |view, _| view.renaming()));
    assert!(view.read_with(cx, |view, _| view.note().is_none()));
    assert!(tree.path().join("Projects").is_dir());
}

#[gpui::test]
async fn rename_needs_one_entry_selected(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("rename");
    let home = TempFolder::tree("rename-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    cx.simulate_keystrokes("ctrl-a");
    cx.run_until_parked();
    assert!(cx.debug_bounds("island-rename").is_none());
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |view, _| view.renaming()));
}

#[gpui::test]
async fn new_folder_makes_one_and_starts_renaming_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("rename");
    let home = TempFolder::tree("rename-home");
    fs::create_dir(tree.path().join("New folder")).unwrap();
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    cx.simulate_keystrokes("ctrl-shift-n");
    wait_for_names(
        &view,
        cx,
        &["New folder", "New folder 2", "Projects", "notes.txt"],
    )
    .await;
    cx.run_until_parked();
    assert_eq!(
        field_state(&view, cx, 1),
        ("New folder 2".to_owned(), 0..12)
    );
    cx.simulate_input("Music");
    cx.simulate_keystrokes("enter");
    wait_for_names(&view, cx, &["Music", "New folder", "Projects", "notes.txt"]).await;
    assert!(tree.path().join("Music").is_dir());
}
