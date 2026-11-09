//! Several entries at once, when the app asks for more than one:
//! Ctrl+click adds or takes out, Shift+click selects a range, the name box
//! shows the files in quotes, and Open picks them all.

use nova_files_api::ListingId;

use super::*;

const SEVERAL_FILES: PickKind = PickKind::Open {
    multiple: true,
    folders: false,
};
const SEVERAL_FOLDERS: PickKind = PickKind::Open {
    multiple: true,
    folders: true,
};

/// The shared tree plus a `docs` folder and `a.txt`, `b.txt`.
fn several_tree() -> TempFolder {
    let tree = TempFolder::tree("several");
    fs::create_dir(tree.path().join("docs")).unwrap();
    fs::write(tree.path().join("a.txt"), "").unwrap();
    fs::write(tree.path().join("b.txt"), "").unwrap();
    tree
}

/// A picker for `kind` on [`several_tree`], checked to show its rows as
/// the tests expect.
async fn several<'a>(
    cx: &'a mut TestAppContext,
    tree: &TempFolder,
    kind: PickKind,
) -> (
    Entity<MainWindow>,
    &'a mut VisualTestContext,
    Receiver<PickAnswer>,
) {
    let (view, cx, answers) = picker(cx, tree, tree.path(), kind).await;
    let rows = view.read_with(cx, |view, _| view.entry_names());
    assert_eq!(rows, ["docs", "Projects", "a.txt", "b.txt", "notes.txt"]);
    (view, cx, answers)
}

// Rows on screen: 0 `docs`, 1 `Projects`, 2 `a.txt`, 3 `b.txt`, 4 `notes.txt`.

fn click_with(cx: &mut VisualTestContext, selector: &'static str, modifiers: Modifiers) {
    cx.run_until_parked();
    let bounds = cx.debug_bounds(selector).unwrap();
    cx.simulate_click(bounds.center(), modifiers);
}

fn ctrl_click(cx: &mut VisualTestContext, selector: &'static str) {
    click_with(cx, selector, Modifiers::control());
}

fn shift_click(cx: &mut VisualTestContext, selector: &'static str) {
    click_with(cx, selector, Modifiers::shift());
}

/// The rows selected, by index.
fn selected(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Vec<u32> {
    let selection = view.read_with(cx, |view, _| view.pick_selection());
    selection
        .map(|(_, ids)| ids.iter().map(|id| id.0).collect())
        .unwrap_or_default()
}

fn picked_all(tree: &TempFolder, names: &[&str]) -> PickAnswer {
    PickAnswer::Picked {
        paths: names.iter().map(|name| tree.path().join(name)).collect(),
        filter: None,
    }
}

#[gpui::test]
async fn ctrl_click_adds_files_and_open_picks_them_all(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = several_tree();
    let (view, cx, answers) = several(cx, &tree, SEVERAL_FILES).await;
    click(cx, "entry-4");
    ctrl_click(cx, "entry-2");
    assert_eq!(selected(&view, cx), [2, 4]);
    assert_eq!(name(&view, cx).as_deref(), Some("\"a.txt\" \"notes.txt\""));
    view.read_with(cx, |view, _| {
        let listing = view.listing_id().unwrap();
        let shown: Vec<bool> = (0..5)
            .map(|row| view.pick_selects((listing, EntryId(row))))
            .collect();
        assert_eq!(shown, [false, false, true, false, true]);
        assert!(!view.pick_selects((ListingId(listing.0 + 1), EntryId(2))));
    });
    click(cx, "pick-accept");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked_all(&tree, &["a.txt", "notes.txt"]))
    );
    assert!(closed(cx));
}

#[gpui::test]
async fn shift_click_selects_a_range_and_enter_picks_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = several_tree();
    let (view, cx, answers) = several(cx, &tree, SEVERAL_FILES).await;
    click(cx, "entry-4");
    shift_click(cx, "entry-2");
    assert_eq!(selected(&view, cx), [2, 3, 4]);
    assert_eq!(
        name(&view, cx).as_deref(),
        Some("\"a.txt\" \"b.txt\" \"notes.txt\"")
    );
    cx.simulate_keystrokes("enter");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked_all(&tree, &["a.txt", "b.txt", "notes.txt"]))
    );
}

#[gpui::test]
async fn ctrl_click_takes_a_file_back_out(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = several_tree();
    let (view, cx, answers) = several(cx, &tree, SEVERAL_FILES).await;
    click(cx, "entry-2");
    ctrl_click(cx, "entry-3");
    ctrl_click(cx, "entry-2");
    assert_eq!(selected(&view, cx), [3]);
    assert_eq!(name(&view, cx).as_deref(), Some("b.txt"));
    ctrl_click(cx, "entry-3");
    assert_eq!(view.read_with(cx, |view, _| view.pick_selection()), None);
    assert_eq!(name(&view, cx).as_deref(), Some(""));
    // Nothing selected and nothing typed: nothing to open.
    cx.simulate_keystrokes("enter");
    assert!(answers.try_recv().is_err());
    ctrl_click(cx, "entry-3");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked(tree.path().join("b.txt")))
    );
}

#[gpui::test]
async fn with_the_box_emptied_open_picks_the_files_selected(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = several_tree();
    let (view, cx, answers) = several(cx, &tree, SEVERAL_FILES).await;
    click(cx, "entry-2");
    ctrl_click(cx, "entry-3");
    cx.simulate_keystrokes("ctrl-a backspace");
    assert_eq!(name(&view, cx).as_deref(), Some(""));
    cx.simulate_keystrokes("enter");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked_all(&tree, &["a.txt", "b.txt"]))
    );
}

#[gpui::test]
async fn edited_names_are_read_as_typed(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = several_tree();
    let (view, cx, answers) = several(cx, &tree, SEVERAL_FILES).await;
    click(cx, "entry-2");
    ctrl_click(cx, "entry-3");
    cx.simulate_keystrokes("end");
    cx.simulate_input("x");
    cx.simulate_keystrokes("enter");
    // No file has that name, so nothing opens.
    assert!(answers.try_recv().is_err());
    assert!(view.read_with(cx, |view, _| view.pick_id().is_some()));
}

#[gpui::test]
async fn a_double_click_picks_just_that_file(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = several_tree();
    let (_view, cx, answers) = several(cx, &tree, SEVERAL_FILES).await;
    click(cx, "entry-2");
    ctrl_click(cx, "entry-3");
    double_click(cx, "entry-4");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked(tree.path().join("notes.txt")))
    );
}

#[gpui::test]
async fn keys_held_do_nothing_where_the_app_wants_one_file(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = several_tree();
    let (view, cx, _answers) = several(cx, &tree, FILES).await;
    click(cx, "entry-2");
    ctrl_click(cx, "entry-3");
    assert_eq!(selected(&view, cx), [3]);
    shift_click(cx, "entry-4");
    assert_eq!(selected(&view, cx), [4]);
    assert_eq!(name(&view, cx).as_deref(), Some("notes.txt"));
}

#[gpui::test]
async fn ctrl_click_on_a_folder_keeps_the_files_selected(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = several_tree();
    let (view, cx, _answers) = several(cx, &tree, SEVERAL_FILES).await;
    click(cx, "entry-2");
    ctrl_click(cx, "entry-0");
    shift_click(cx, "entry-1");
    let folder = view.read_with(cx, |view, _| view.folder().map(Path::to_path_buf));
    assert_eq!(folder.as_deref(), Some(tree.path()));
    assert!(!view.read_with(cx, |view, _| view.waiting()));
    assert_eq!(selected(&view, cx), [2]);
    assert_eq!(name(&view, cx).as_deref(), Some("a.txt"));
}

#[gpui::test]
async fn a_folder_picker_picks_several_folders(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = several_tree();
    let (view, cx, answers) = several(cx, &tree, SEVERAL_FOLDERS).await;
    click(cx, "entry-1");
    shift_click(cx, "entry-0");
    // Files aren't what this picker picks.
    ctrl_click(cx, "entry-3");
    assert_eq!(selected(&view, cx), [0, 1]);
    assert_eq!(name(&view, cx).as_deref(), Some(""));
    ctrl_click(cx, "entry-1");
    assert_eq!(selected(&view, cx), [0]);
    ctrl_click(cx, "entry-1");
    click(cx, "pick-accept");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked_all(&tree, &["docs", "Projects"]))
    );
}
