//! The quick look in a main window on a real core: Space opens it on the
//! selected entry and closes it, Escape closes it, the arrow keys step
//! through the folder with the selection following, Enter opens what it
//! shows, and going to another folder closes it.

use super::*;

/// The shared tree plus `a.txt` with some text: Projects, a.txt,
/// notes.txt.
async fn with_text<'a>(
    cx: &'a mut TestAppContext,
    tree: &TempFolder,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    fs::write(tree.path().join("a.txt"), "first line\nsecond line\n").unwrap();
    cx.update(nova_files_ui::bind_window_keys);
    let (view, cx) = open_on(cx, tree.path()).await;
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.entry_names(), ["Projects", "a.txt", "notes.txt"]);
    });
    (view, cx)
}

/// Waits until the quick look shows what the core sent for `name`.
async fn wait_for_preview(view: &Entity<MainWindow>, cx: &VisualTestContext, name: &str) {
    view.condition::<WindowEvent>(cx, |view, _| {
        view.previewed()
            .is_some_and(|shown| shown.name == name && shown.preview.is_some())
    })
    .await;
}

fn previewed_name(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Option<String> {
    view.read_with(cx, |view, _| view.previewed().map(|shown| shown.name))
}

fn selected(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, _| view.selected_names())
}

#[gpui::test]
async fn space_shows_the_selected_file_and_closes_it_again(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    // Nothing selected: nothing to show.
    cx.simulate_keystrokes("space");
    assert!(!view.read_with(cx, |view, _| view.previewing()));

    click(cx, "entry-1");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "a.txt").await;
    let preview = view.read_with(cx, |view, _| view.previewed().unwrap().preview.unwrap());
    assert_eq!(
        preview.content,
        nova_files_api::PreviewContent::Text {
            text: "first line\nsecond line\n".into(),
            cut: false
        }
    );
    cx.run_until_parked();
    assert!(cx.debug_bounds("preview").is_some());

    cx.simulate_keystrokes("space");
    assert_eq!(previewed_name(&view, cx), None);
    cx.run_until_parked();
    assert!(cx.debug_bounds("preview").is_none());
    // The selection stays.
    assert_eq!(selected(&view, cx), ["a.txt"]);
}

#[gpui::test]
async fn escape_closes_the_quick_look_and_keeps_the_selection(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    click(cx, "entry-1");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "a.txt").await;
    cx.simulate_keystrokes("escape");
    assert_eq!(previewed_name(&view, cx), None);
    assert_eq!(selected(&view, cx), ["a.txt"]);
}

#[gpui::test]
async fn the_arrow_keys_step_through_the_folder_and_the_selection_follows(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    click(cx, "entry-1");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "a.txt").await;

    cx.simulate_keystrokes("right");
    wait_for_preview(&view, cx, "notes.txt").await;
    assert_eq!(selected(&view, cx), ["notes.txt"]);
    // The last entry: Next goes nowhere.
    cx.simulate_keystrokes("down");
    assert_eq!(previewed_name(&view, cx).as_deref(), Some("notes.txt"));

    cx.simulate_keystrokes("up left");
    wait_for_preview(&view, cx, "Projects").await;
    assert_eq!(selected(&view, cx), ["Projects"]);
    let preview = view.read_with(cx, |view, _| view.previewed().unwrap().preview.unwrap());
    match &preview.content {
        nova_files_api::PreviewContent::Folder { first, count } => {
            assert_eq!(*count, 1);
            assert_eq!(first[0].name, "nova");
        }
        other => panic!("expected a folder, got {other:?}"),
    }
}

#[gpui::test]
async fn the_buttons_step_and_close(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    click(cx, "entry-1");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "a.txt").await;
    click(cx, "preview-next");
    wait_for_preview(&view, cx, "notes.txt").await;
    click(cx, "preview-previous");
    wait_for_preview(&view, cx, "a.txt").await;
    // A click on what is shown leaves it open.
    click(cx, "preview-art");
    assert!(view.read_with(cx, |view, _| view.previewing()));
    click(cx, "preview-close");
    assert!(!view.read_with(cx, |view, _| view.previewing()));
}

#[gpui::test]
async fn enter_on_a_folder_goes_into_it_and_closes_the_quick_look(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    click(cx, "entry-0");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "Projects").await;
    cx.simulate_keystrokes("enter");
    assert!(!view.read_with(cx, |view, _| view.previewing()));
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;
}

#[gpui::test]
async fn going_to_another_folder_closes_the_quick_look(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    click(cx, "entry-1");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "a.txt").await;
    cx.simulate_keystrokes("alt-up");
    view.condition::<WindowEvent>(cx, |view, _| view.folder() != Some(tree.path()))
        .await;
    assert!(!view.read_with(cx, |view, _| view.previewing()));
}

#[gpui::test]
async fn a_click_beside_what_is_shown_closes_the_quick_look(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    click(cx, "entry-1");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "a.txt").await;
    cx.run_until_parked();
    let scrim = cx.debug_bounds("preview").unwrap();
    cx.simulate_click(
        scrim.origin + gpui::point(gpui::px(4.0), gpui::px(4.0)),
        Modifiers::none(),
    );
    assert!(!view.read_with(cx, |view, _| view.previewing()));
}

#[gpui::test]
async fn a_fresh_read_keeps_the_quick_look_on_its_entry(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    click(cx, "entry-2");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "notes.txt").await;
    // Ordered Z to A, notes.txt moves; the quick look follows it. (Its
    // scrim covers the column heads, so the test asks for the order.)
    view.update(cx, |view, cx| {
        view.sort_by(nova_files_api::SortKey::Name);
        cx.notify();
    });
    view.condition::<WindowEvent>(cx, |view, _| {
        view.entry_names() == ["Projects", "notes.txt", "a.txt"]
    })
    .await;
    assert_eq!(previewed_name(&view, cx).as_deref(), Some("notes.txt"));
    cx.simulate_keystrokes("right");
    wait_for_preview(&view, cx, "a.txt").await;
}

#[gpui::test]
async fn any_cursor_move_takes_the_quick_look_along(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    click(cx, "entry-1");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "a.txt").await;
    cx.simulate_keystrokes("end");
    wait_for_preview(&view, cx, "notes.txt").await;
    cx.simulate_keystrokes("home");
    wait_for_preview(&view, cx, "Projects").await;
    assert_eq!(selected(&view, cx), ["Projects"]);
}

#[gpui::test]
async fn a_fresh_read_shows_the_file_as_it_is_now(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    click(cx, "entry-1");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "a.txt").await;
    fs::write(tree.path().join("a.txt"), "changed\n").unwrap();
    view.update(cx, |view, cx| {
        view.sort_by(nova_files_api::SortKey::Name);
        cx.notify();
    });
    view.condition::<WindowEvent>(cx, |view, _| {
        view.previewed()
            .and_then(|shown| shown.preview)
            .is_some_and(|preview| {
                preview.content
                    == nova_files_api::PreviewContent::Text {
                        text: "changed\n".into(),
                        cut: false,
                    }
            })
    })
    .await;
}
