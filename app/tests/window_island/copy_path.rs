use gpui::point;

use super::menus::choose;
use super::*;
use crate::window_sidebar::right_click_at;

fn clipboard_text(cx: &VisualTestContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|item| item.text())
}

#[gpui::test]
async fn the_key_copies_the_selected_paths_or_the_folder_as_text(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("copy-path-key");
    let home = TempFolder::tree("copy-path-key-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    cx.simulate_keystrokes("ctrl-shift-c");
    wait_for_note(&view, cx, "Copied path").await;
    assert_eq!(clipboard_text(cx), Some(tree.path().display().to_string()));

    click(cx, "name-0");
    click_with(cx, "name-1", Modifiers::control());
    cx.simulate_keystrokes("ctrl-shift-c");
    wait_for_note(&view, cx, "Copied 2 paths").await;
    let both = format!(
        "{}\n{}",
        tree.path().join("Projects").display(),
        tree.path().join("notes.txt").display()
    );
    assert_eq!(clipboard_text(cx), Some(both));
}

#[gpui::test]
async fn the_space_menu_copies_the_folder_even_with_a_selection(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("copy-path-menu");
    let home = TempFolder::tree("copy-path-menu-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-1");
    let last = cx.debug_bounds("entry-1").unwrap();
    right_click_at(
        cx,
        point(last.center().x, last.bottom() + last.size.height * 2.0),
    );
    choose(&view, cx, "Copy path");
    wait_for_note(&view, cx, "Copied path").await;
    assert_eq!(clipboard_text(cx), Some(tree.path().display().to_string()));
}
