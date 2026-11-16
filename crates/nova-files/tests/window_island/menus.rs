//! The sheet's right-click menus on a real core: what an entry, several
//! entries, the empty space and the trash offer, that each item does what
//! its key does, and the Menu key.

use gpui::{Pixels, Point, point};
use nova_files_ui::{Look, MenuAction};

use super::*;
use crate::window_sidebar::{right_click, right_click_at};

/// The labels of the menu on screen, and the items a line sits above.
fn offered(view: &Entity<MainWindow>, cx: &VisualTestContext) -> (Vec<String>, Vec<usize>) {
    view.read_with(cx, |view, _| {
        let menu = view.menu().unwrap();
        let labels = menu
            .items
            .iter()
            .map(|item| item.label.to_string())
            .collect();
        (labels, menu.lines.clone())
    })
}

/// Right-clicks the sheet's empty space, under the last of `rows` rows.
fn right_click_space(cx: &mut VisualTestContext, rows: usize) {
    let at = below_rows(cx, rows);
    right_click_at(cx, at);
}

/// A point on the sheet's empty space, under the last of `rows` rows.
fn below_rows(cx: &mut VisualTestContext, rows: usize) -> Point<Pixels> {
    cx.run_until_parked();
    let selector: &'static str = match rows {
        1 => "entry-0",
        2 => "entry-1",
        _ => "entry-2",
    };
    let last = cx.debug_bounds(selector).unwrap();
    point(last.center().x, last.bottom() + last.size.height * 2.0)
}

/// Clicks the item of the menu on screen labelled `label`.
fn choose(view: &Entity<MainWindow>, cx: &mut VisualTestContext, label: &str) {
    let index = view.read_with(cx, |view, _| {
        view.menu()
            .unwrap()
            .items
            .iter()
            .position(|item| item.label == label)
            .unwrap()
    });
    let selector: &'static str =
        ["menu-0", "menu-1", "menu-2", "menu-3", "menu-4", "menu-5"][index];
    click(cx, selector);
}

#[gpui::test]
async fn an_entry_offers_its_actions_and_each_does_what_its_key_does(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("menu-entry");
    let home = TempFolder::tree("menu-entry-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    // A right-click selects the entry it is on.
    right_click(cx, "name-1");
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_names()),
        ["notes.txt"]
    );
    assert_eq!(
        offered(&view, cx),
        (
            ["Open", "Preview", "Cut", "Copy", "Rename", "Move to Trash"]
                .map(str::to_owned)
                .to_vec(),
            vec![2, 5]
        )
    );
    view.read_with(cx, |view, _| {
        let menu = view.menu().unwrap();
        assert_eq!(menu.items[5].look, Look::Danger);
        assert!(matches!(menu.items[3].action, MenuAction::Run(_)));
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu-line-2").is_some());

    choose(&view, cx, "Copy");
    wait_for_note(&view, cx, "Copied notes.txt").await;
    assert!(view.read_with(cx, |view, _| view.menu().is_none()));

    // The empty space: New folder and Paste, Select all, hidden files.
    right_click_space(cx, 2);
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_count()),
        1,
        "a right-click on the space leaves the selection"
    );
    assert_eq!(
        offered(&view, cx),
        (
            ["New folder", "Paste", "Select all", "Show hidden files"]
                .map(str::to_owned)
                .to_vec(),
            vec![2, 3]
        )
    );
    choose(&view, cx, "Paste");
    view.condition::<WindowEvent>(cx, |view, _| view.entry_names().len() == 3)
        .await;

    right_click_space(cx, 3);
    choose(&view, cx, "Select all");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.selected_count()), 3);
}

#[gpui::test]
async fn several_entries_offer_only_what_applies_to_all(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("menu-several");
    let home = TempFolder::tree("menu-several-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;
    click(cx, "name-0");
    click_with(cx, "name-1", Modifiers::secondary_key());

    // Inside the selection, a right-click keeps it.
    right_click(cx, "name-0");
    assert_eq!(view.read_with(cx, |view, _| view.selected_count()), 2);
    assert_eq!(
        offered(&view, cx),
        (
            ["Cut", "Copy", "Move to Trash"].map(str::to_owned).to_vec(),
            vec![2]
        )
    );

    choose(&view, cx, "Move to Trash");
    wait_for_names(&view, cx, &[]).await;
    assert!(!tree.path().join("notes.txt").exists());
}

#[gpui::test]
async fn the_menu_key_opens_the_menu_with_its_first_item_marked(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("menu-key");
    let home = TempFolder::tree("menu-key-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    // Nothing selected: the empty space's menu.
    cx.simulate_keystrokes("shift-f10");
    assert_eq!(offered(&view, cx).0[0], "New folder");
    assert_eq!(
        view.read_with(cx, |view, _| view.menu().unwrap().active),
        Some(0)
    );
    cx.simulate_keystrokes("escape");
    assert!(view.read_with(cx, |view, _| view.menu().is_none()));

    // Projects selected: Enter on Open goes into it.
    click(cx, "name-0");
    cx.simulate_keystrokes("menu");
    assert_eq!(offered(&view, cx).0[0], "Open");
    cx.simulate_keystrokes("enter");
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;
}

#[gpui::test]
async fn show_hidden_files_is_ticked_while_they_show(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("menu-hidden");
    let home = TempFolder::tree("menu-hidden-home");
    fs::write(tree.path().join(".secret"), "").unwrap();
    let (view, cx) = island_window(cx, tree.path(), &home).await;
    let hidden_item = |view: &Entity<MainWindow>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            let menu = view.menu().unwrap();
            menu.items.last().unwrap().look
        })
    };

    right_click_space(cx, 2);
    assert_eq!(hidden_item(&view, cx), Look::Plain);
    choose(&view, cx, "Show hidden files");
    wait_for_names(&view, cx, &["Projects", ".secret", "notes.txt"]).await;

    right_click_space(cx, 3);
    assert_eq!(hidden_item(&view, cx), Look::On);
    choose(&view, cx, "Show hidden files");
    wait_for_names(&view, cx, &["Projects", "notes.txt"]).await;
}

#[gpui::test]
async fn the_trash_offers_restore_and_empty_trash(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("menu-trash");
    let home = TempFolder::tree("menu-trash-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;
    click(cx, "name-1");
    cx.simulate_keystrokes("delete");
    wait_for_note(&view, cx, "Moved notes.txt to Trash").await;
    view.update(cx, MainWindow::open_trash);
    wait_for_names(&view, cx, &["notes.txt"]).await;

    right_click_space(cx, 1);
    assert_eq!(
        offered(&view, cx),
        (
            ["Empty Trash", "Select all"].map(str::to_owned).to_vec(),
            vec![1]
        )
    );
    cx.simulate_keystrokes("escape");

    right_click(cx, "name-0");
    assert_eq!(
        offered(&view, cx),
        (["Restore"].map(str::to_owned).to_vec(), vec![])
    );
    choose(&view, cx, "Restore");
    wait_for_names(&view, cx, &[]).await;
    assert!(tree.path().join("notes.txt").is_file());
}
