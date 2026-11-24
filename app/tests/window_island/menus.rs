use gpui::{Pixels, Point, point, px};
use nova_files_api::{FolderSizes, SortKey};
use nova_files_window::{Look, MenuAction};

use super::*;
use crate::window_sidebar::{right_click, right_click_at};

pub(super) fn offered(
    view: &Entity<MainWindow>,
    cx: &VisualTestContext,
) -> (Vec<String>, Vec<usize>) {
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

fn right_click_space(cx: &mut VisualTestContext, rows: usize) {
    let at = below_rows(cx, rows);
    right_click_at(cx, at);
}

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

pub(super) fn choose(view: &Entity<MainWindow>, cx: &mut VisualTestContext, label: &str) {
    let index = view.read_with(cx, |view, _| {
        view.menu()
            .unwrap()
            .items
            .iter()
            .position(|item| item.label == label)
            .unwrap()
    });
    let selector: &'static str = [
        "menu-0", "menu-1", "menu-2", "menu-3", "menu-4", "menu-5", "menu-6", "menu-7", "menu-8",
        "menu-9", "menu-10",
    ][index];
    click(cx, selector);
}

#[gpui::test]
async fn an_entry_offers_its_actions_and_each_does_what_its_key_does(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("menu-entry");
    let home = TempFolder::tree("menu-entry-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    right_click(cx, "name-1");
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_names()),
        ["notes.txt"]
    );
    assert_eq!(
        offered(&view, cx),
        (
            [
                "Open",
                "Preview",
                "Cut",
                "Copy",
                "Copy path",
                "Rename",
                "Compress",
                "Compress as…",
                "Move to Trash",
                "Properties"
            ]
            .map(str::to_owned)
            .to_vec(),
            vec![2, 8, 9]
        )
    );
    view.read_with(cx, |view, _| {
        let menu = view.menu().unwrap();
        assert_eq!(menu.items[8].look, Look::Danger);
        assert!(matches!(menu.items[3].action, MenuAction::Run(_)));
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu-line-2").is_some());

    choose(&view, cx, "Copy");
    wait_for_note(&view, cx, "Copied notes.txt").await;
    assert!(view.read_with(cx, |view, _| view.menu().is_none()));

    right_click_space(cx, 2);
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_count()),
        1,
        "a right-click on the space leaves the selection"
    );
    assert_eq!(
        offered(&view, cx),
        (
            [
                "New folder",
                "New file",
                "Paste",
                "Copy path",
                "Open terminal here",
                "Select all",
                "Details",
                "Icons",
                "Gallery",
                "Show hidden files",
                "Folder sizes"
            ]
            .map(str::to_owned)
            .to_vec(),
            vec![5, 6, 9]
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
    click_with(cx, "name-1", Modifiers::control());

    right_click(cx, "name-0");
    assert_eq!(view.read_with(cx, |view, _| view.selected_count()), 2);
    assert_eq!(
        offered(&view, cx),
        (
            [
                "Cut",
                "Copy",
                "Copy path",
                "Compress",
                "Compress as…",
                "Move to Trash",
                "Properties"
            ]
            .map(str::to_owned)
            .to_vec(),
            vec![5, 6]
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

    cx.simulate_keystrokes("shift-f10");
    assert_eq!(
        offered(&view, cx).0,
        [
            "New folder",
            "New file",
            "Copy path",
            "Open terminal here",
            "Select all",
            "Details",
            "Icons",
            "Gallery",
            "Show hidden files",
            "Folder sizes"
        ]
    );
    assert_eq!(
        view.read_with(cx, |view, _| view.menu().unwrap().active),
        Some(0)
    );
    cx.simulate_keystrokes("escape");
    assert!(view.read_with(cx, |view, _| view.menu().is_none()));

    click(cx, "name-0");
    cx.simulate_keystrokes("menu");
    assert_eq!(offered(&view, cx).0[0], "Open");
    cx.simulate_keystrokes("enter");
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;

    right_click(cx, "name-0");
    assert!(view.read_with(cx, |view, _| view.menu().is_some()));
    cx.simulate_keystrokes("alt-up");
    wait_for_folder(&view, cx, tree.path()).await;
    assert!(view.read_with(cx, |view, _| view.menu().is_none()));
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
            let item = menu
                .items
                .iter()
                .find(|item| item.label == "Show hidden files");
            item.unwrap().look
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
async fn folder_sizes_is_ticked_until_turned_off(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("menu-folder-sizes");
    let home = TempFolder::tree("menu-folder-sizes-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;
    let sizes_item = |view: &Entity<MainWindow>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            let menu = view.menu().unwrap();
            let item = menu.items.iter().find(|item| item.label == "Folder sizes");
            item.unwrap().look
        })
    };

    right_click_space(cx, 2);
    assert_eq!(sizes_item(&view, cx), Look::On);
    choose(&view, cx, "Folder sizes");
    view.condition::<WindowEvent>(cx, |_, cx| {
        Published::<FolderSizes>::get(cx) == Some(&FolderSizes::Left)
    })
    .await;

    right_click_space(cx, 2);
    assert_eq!(sizes_item(&view, cx), Look::Plain);
}

#[gpui::test]
async fn the_view_showing_is_ticked_and_another_can_be_picked(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("menu-views");
    let home = TempFolder::tree("menu-views-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;
    let looks = |view: &Entity<MainWindow>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            let menu = view.menu().unwrap();
            ["Details", "Icons", "Gallery"].map(|label| {
                let item = menu.items.iter().find(|item| item.label == label);
                item.unwrap().look
            })
        })
    };

    right_click_space(cx, 2);
    assert_eq!(looks(&view, cx), [Look::On, Look::Plain, Look::Plain]);
    choose(&view, cx, "Icons");
    cx.run_until_parked();

    right_click_space(cx, 2);
    assert_eq!(looks(&view, cx), [Look::Plain, Look::On, Look::Plain]);
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
    open_trash(&view, cx, false).await;
    wait_for_names(&view, cx, &["notes.txt"]).await;
    view.condition::<WindowEvent>(cx, |view, cx| trash_count(view, cx) == Some(1))
        .await;

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
        (
            ["Open", "Preview", "Restore", "Delete for good"]
                .map(str::to_owned)
                .to_vec(),
            vec![2, 3]
        )
    );
    view.read_with(cx, |view, _| {
        assert_eq!(view.menu().unwrap().items[3].look, Look::Danger);
    });
    choose(&view, cx, "Restore");
    wait_for_names(&view, cx, &[]).await;
    assert!(tree.path().join("notes.txt").is_file());
}

#[gpui::test]
async fn everything_in_the_trash_can_be_deleted_for_good_from_the_menu(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("menu-trash-delete");
    let home = TempFolder::tree("menu-trash-delete-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;
    click(cx, "name-0");
    cx.simulate_keystrokes("ctrl-a delete");
    wait_for_note(&view, cx, "Moved 2 items to Trash").await;
    open_trash(&view, cx, false).await;
    wait_for_names(&view, cx, &["Projects", "notes.txt"]).await;
    view.condition::<WindowEvent>(cx, |view, cx| trash_count(view, cx) == Some(2))
        .await;

    cx.simulate_keystrokes("ctrl-a");
    right_click(cx, "name-0");
    assert_eq!(view.read_with(cx, |view, _| view.selected_count()), 2);
    assert_eq!(
        offered(&view, cx),
        (
            ["Restore", "Delete for good"].map(str::to_owned).to_vec(),
            vec![1]
        )
    );
    choose(&view, cx, "Delete for good");
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.confirm().is_none()));
    wait_for_names(&view, cx, &[]).await;
    view.condition::<WindowEvent>(cx, |view, cx| trash_count(view, cx) == Some(0))
        .await;
}

#[gpui::test]
async fn inside_a_trashed_folder_files_only_open(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("menu-trashed");
    let home = TempFolder::tree("menu-trashed-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;
    click(cx, "name-0");
    cx.simulate_keystrokes("delete");
    wait_for_note(&view, cx, "Moved Projects to Trash").await;
    open_trash(&view, cx, false).await;
    wait_for_names(&view, cx, &["Projects"]).await;
    double_click(cx, "name-0");
    wait_for_names(&view, cx, &["nova"]).await;

    right_click(cx, "name-0");
    assert_eq!(
        offered(&view, cx),
        (
            ["Open", "Open in new tab", "Preview"]
                .map(str::to_owned)
                .to_vec(),
            vec![]
        )
    );
    cx.simulate_keystrokes("escape");
    right_click_space(cx, 1);
    assert_eq!(
        offered(&view, cx),
        (["Select all"].map(str::to_owned).to_vec(), vec![])
    );
}

#[gpui::test]
async fn a_folder_opens_in_a_new_tab_from_its_menu(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("menu-new-tab");
    let home = TempFolder::tree("menu-new-tab-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    right_click(cx, "name-0");
    assert_eq!(
        offered(&view, cx).0,
        [
            "Open",
            "Open in new tab",
            "Preview",
            "Cut",
            "Copy",
            "Copy path",
            "Rename",
            "Compress",
            "Compress as…",
            "Move to Trash",
            "Properties"
        ]
    );
    choose(&view, cx, "Open in new tab");
    let projects = tree.path().join("Projects");
    wait_for_folder(&view, cx, &projects).await;
    assert_eq!(
        view.read_with(cx, |view, _| view.tab_places()),
        [
            Some(Location::Folder(tree.path().to_owned())),
            Some(Location::Folder(projects))
        ]
    );
}

#[gpui::test]
async fn an_empty_trash_offers_nothing(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("menu-trash-none");
    let home = TempFolder::tree("menu-trash-none-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;
    open_trash(&view, cx, false).await;
    view.condition::<WindowEvent>(cx, |view, cx| trash_count(view, cx) == Some(0))
        .await;
    wait_for_names(&view, cx, &[]).await;
    cx.simulate_keystrokes("shift-f10");
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.menu().is_none()));
}

fn menu_at(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Point<Pixels> {
    view.read_with(cx, |view, _| view.menu().unwrap().at)
}

#[gpui::test]
async fn the_menu_key_opens_at_the_list_or_under_the_row(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("menu-place");
    let home = TempFolder::tree("menu-place-home");
    for index in 0..60 {
        fs::write(tree.path().join(format!("file-{index:02}")), "").unwrap();
    }
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    cx.simulate_keystrokes("shift-f10");
    let first = cx.debug_bounds("entry-0").unwrap();
    let at = menu_at(&view, cx);
    assert!(
        at.x > first.left() && at.y > first.top() && at.y < first.bottom(),
        "{at:?} {first:?}"
    );
    cx.simulate_keystrokes("escape");

    click(cx, "name-1");
    cx.simulate_keystrokes("menu");
    let row = cx.debug_bounds("entry-1").unwrap();
    let at = menu_at(&view, cx);
    assert!(
        at.x > row.left() && (at.y - row.bottom()).abs() < px(0.5),
        "{at:?} {row:?}"
    );
    cx.simulate_keystrokes("escape end up up up menu");
    cx.run_until_parked();
    let row = cx.debug_bounds("entry-58").unwrap();
    let at = menu_at(&view, cx);
    assert!((at.y - row.bottom()).abs() < px(0.5), "{at:?} {row:?}");
}

#[gpui::test]
async fn sorting_by_size_with_folder_sizes_sorts_folders_and_files_by_what_they_hold(
    cx: &mut TestAppContext,
) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("menu-sizes-sorted");
    let home = TempFolder::tree("menu-sizes-sorted-home");
    fs::write(tree.path().join("Projects/nova/data"), [0_u8; 4000]).unwrap();
    fs::write(tree.path().join("notes.txt"), [0_u8; 100]).unwrap();
    fs::create_dir(tree.path().join("Empty")).unwrap();
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    view.condition::<WindowEvent>(cx, |_, cx| {
        Published::<FolderSizes>::get(cx) == Some(&FolderSizes::Measured)
    })
    .await;
    view.update(cx, |view, _| view.sort_by(SortKey::Size));
    wait_for_names(&view, cx, &["Projects", "notes.txt", "Empty"]).await;
    cx.run_until_parked();

    view.read_with(cx, |view, _| {
        assert!(view.shows_shares());
        let near = |index, want: f32| {
            view.share(index)
                .is_some_and(|got| (got - want).abs() < 1e-6)
        };
        assert!(near(0, 1.0));
        assert!(near(1, 0.025));
    });
}

#[gpui::test]
async fn a_menu_near_the_bottom_right_opens_up_and_left_of_the_pointer(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("menu-corner");
    let home = TempFolder::tree("menu-corner-home");
    let (_view, cx) = island_window(cx, tree.path(), &home).await;

    let window = cx.update(|window, _| window.viewport_size());
    let at = point(window.width - px(40.0), window.height - px(120.0));
    right_click_at(cx, at);
    cx.run_until_parked();
    let menu = cx.debug_bounds("menu").unwrap();
    assert!(
        menu.bottom() <= at.y && menu.right() <= at.x,
        "{menu:?} {at:?}"
    );

    cx.simulate_keystrokes("escape");
    let first = cx.debug_bounds("entry-0").unwrap().center();
    right_click_at(cx, first);
    cx.run_until_parked();
    let menu = cx.debug_bounds("menu").unwrap();
    assert!(
        menu.top() >= first.y && menu.left() >= first.x,
        "{menu:?} {first:?}"
    );
}

#[gpui::test]
async fn while_a_menu_is_open_the_list_stays_and_a_click_only_closes_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("menu-open");
    let home = TempFolder::tree("menu-open-home");
    for index in 0..60 {
        fs::write(tree.path().join(format!("file-{index:02}")), "").unwrap();
    }
    let (view, cx) = island_window(cx, tree.path(), &home).await;
    let offset = |view: &Entity<MainWindow>, cx: &VisualTestContext| {
        view.read_with(cx, |view, _| view.scroll().0.borrow().base_handle.offset())
    };

    right_click(cx, "entry-1");
    assert!(view.read_with(cx, |view, _| view.menu().is_some()));
    let row = cx.debug_bounds("entry-6").unwrap();
    let over = point(row.left() + px(60.0), row.center().y);
    cx.simulate_event(gpui::ScrollEvent {
        position: over,
        delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(-300.0))),
        ..gpui::ScrollEvent::default()
    });
    cx.run_until_parked();
    assert_eq!(offset(&view, cx), point(px(0.0), px(0.0)), "the list moved");

    let row = cx.debug_bounds("entry-4").unwrap();
    let other = point(row.left() + px(60.0), row.center().y);
    cx.simulate_click(other, Modifiers::none());
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.menu().is_none()));
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_names()),
        ["file-00"]
    );

    right_click(cx, "entry-1");
    let row = cx.debug_bounds("entry-3").unwrap();
    right_click_at(cx, point(row.left() + px(60.0), row.center().y));
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.menu().is_some()));
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_names()),
        ["file-02"]
    );
}

#[gpui::test]
async fn a_letter_marks_the_next_item_starting_with_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("menu-letter");
    let home = TempFolder::tree("menu-letter-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;
    let marked = |view: &Entity<MainWindow>, cx: &VisualTestContext| {
        view.read_with(cx, |view, _| view.menu().unwrap().active)
    };

    cx.simulate_keystrokes("shift-f10");
    cx.simulate_keystrokes("d");
    assert_eq!(marked(&view, cx), Some(5), "Details");
    cx.simulate_keystrokes("s");
    assert_eq!(marked(&view, cx), Some(8), "Show hidden files");
    cx.simulate_keystrokes("s");
    assert_eq!(marked(&view, cx), Some(4), "round again to Select all");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_names()),
        ["Projects", "notes.txt"]
    );
}

#[gpui::test]
async fn a_right_press_slid_to_an_item_and_let_go_chooses_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("menu-slide");
    let home = TempFolder::tree("menu-slide-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;
    let at = below_rows(cx, 2);

    cx.simulate_mouse_down(at, gpui::MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.menu().is_some()));
    cx.simulate_mouse_up(at, gpui::MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    assert!(
        view.read_with(cx, |view, _| view.menu().is_some()),
        "letting go where it opened keeps it open"
    );
    let select_all = cx.debug_bounds("menu-4").unwrap().center();
    cx.simulate_mouse_move(select_all, None, Modifiers::none());
    cx.simulate_mouse_up(select_all, gpui::MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.menu().is_none()));
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_names()),
        ["Projects", "notes.txt"]
    );
}
