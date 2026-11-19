use std::cell::RefCell;
use std::rc::Rc;

use gpui::{MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, size};

use super::*;

const NARROWEST_WIDE: Pixels = px(720.0);

async fn wait_for_tree(view: &Entity<MainWindow>, cx: &VisualTestContext) {
    view.condition::<WindowEvent>(cx, |view, cx| tree_part(view).read(cx).settled())
        .await;
    cx.run_until_parked();
}

async fn with_projects_pinned<'a>(
    cx: &'a mut TestAppContext,
    tree: &TempFolder,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    let bookmarks = tree.path().join(".config/gtk-3.0");
    fs::create_dir_all(&bookmarks).unwrap();
    let projects = tree.path().join("Projects");
    fs::write(
        bookmarks.join("bookmarks"),
        format!("file://{}\n", projects.display()),
    )
    .unwrap();
    let (view, cx) = window_on(cx, tree.path(), tree.path());
    wait_for_folder(&view, cx, tree.path()).await;
    view.condition::<WindowEvent>(cx, |view, cx| !pinned(view, cx).is_empty())
        .await;
    (view, cx)
}

#[gpui::test]
async fn a_pinned_folder_shows_after_home_and_opens(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("sidebar-pinned");
    let (view, cx) = with_projects_pinned(cx, &tree).await;

    click(cx, "place-1");

    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;
}

fn icon_inset(cx: &mut VisualTestContext) -> Pixels {
    cx.debug_bounds("place-0-here").unwrap().left() - cx.debug_bounds("place-0").unwrap().left()
}

#[gpui::test]
async fn a_narrow_window_folds_places_into_one_row(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("sidebar-narrow");
    let (_view, cx) = with_projects_pinned(cx, &tree).await;
    cx.simulate_resize(size(NARROWEST_WIDE, px(600.0)));
    cx.run_until_parked();
    let (home, pinned) = (
        cx.debug_bounds("place-0").unwrap(),
        cx.debug_bounds("place-1").unwrap(),
    );
    assert!(pinned.top() >= home.bottom(), "wide: one under the other");
    let sheet_left = cx.debug_bounds("modified-header").unwrap().left();
    assert!(home.right() < sheet_left, "wide: beside the sheet");
    assert_eq!(icon_inset(cx), px(10.0));

    cx.simulate_resize(size(NARROWEST_WIDE - px(1.0), px(600.0)));
    cx.run_until_parked();
    let (home, pinned) = (
        cx.debug_bounds("place-0").unwrap(),
        cx.debug_bounds("place-1").unwrap(),
    );
    assert_eq!(home.top(), pinned.top(), "narrow: side by side");
    assert_eq!(icon_inset(cx), px(12.0));
    assert!(pinned.left() >= home.right());
    let header = cx.debug_bounds("modified-header").unwrap();
    assert!(header.top() >= home.bottom(), "narrow: above the sheet");
    let pin = cx.debug_bounds("pin").unwrap();
    assert!(pin.left() >= pinned.right());
}

#[gpui::test]
async fn right_click_on_a_pinned_folder_takes_it_out(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("sidebar-unpin");
    let (view, cx) = with_projects_pinned(cx, &tree).await;
    right_click(cx, "place-0");
    view.read_with(cx, |view, _| assert!(view.menu().is_none()));

    right_click(cx, "place-1");
    view.read_with(cx, |view, _| assert!(view.menu().is_some()));
    right_click(cx, "place-0");
    view.read_with(cx, |view, _| assert!(view.menu().is_none()));

    right_click(cx, "place-1");
    view.read_with(cx, |view, _| {
        let menu = view.menu().unwrap();
        assert_eq!(menu.items.len(), 1);
        assert_eq!(menu.items[0].label, "Remove from Places");
    });
    click(cx, "menu-0");

    view.condition::<WindowEvent>(cx, |view, cx| pinned(view, cx).is_empty())
        .await;
    view.read_with(cx, |view, _| assert!(view.menu().is_none()));
    let saved = fs::read_to_string(tree.path().join(".config/gtk-3.0/bookmarks")).unwrap();
    assert_eq!(saved, "");
}

#[gpui::test]
async fn a_menu_takes_the_keys_until_it_closes(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("sidebar-menu-keys");
    cx.update(nova_files_ui::bind_window_keys);
    let (view, cx) = with_projects_pinned(cx, &tree).await;
    let marked = |view: &Entity<MainWindow>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.menu().map(|menu| menu.active))
    };

    right_click(cx, "place-1");
    assert_eq!(marked(&view, cx), Some(None));
    cx.simulate_keystrokes("down");
    assert_eq!(marked(&view, cx), Some(Some(0)));
    cx.simulate_keystrokes("down");
    assert_eq!(marked(&view, cx), Some(Some(0)));
    cx.simulate_keystrokes("up");
    assert_eq!(marked(&view, cx), Some(Some(0)));
    cx.simulate_keystrokes("escape");
    assert_eq!(marked(&view, cx), None);
    right_click(cx, "place-1");
    cx.simulate_keystrokes("enter");
    assert_eq!(marked(&view, cx), Some(None));

    cx.simulate_keystrokes("down enter");
    view.condition::<WindowEvent>(cx, |view, cx| pinned(view, cx).is_empty())
        .await;
    assert_eq!(marked(&view, cx), None);
}

#[gpui::test]
async fn the_plus_on_places_pins_the_folder_picked(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("sidebar-pin");
    let (view, cx) = window_on(cx, tree.path(), tree.path());
    wait_for_folder(&view, cx, tree.path()).await;
    let windows = cx.windows().len();

    click(cx, "pin");
    let projects = tree.path().join("Projects");
    answer_picker(cx, windows, &projects);

    view.condition::<WindowEvent>(cx, |view, cx| {
        pinned(view, cx).iter().any(|pin| pin.path == projects)
    })
    .await;
}

#[gpui::test]
async fn a_part_asks_for_a_folder_and_gets_the_one_picked(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("sidebar-part-pick");
    let (view, cx) = window_on(cx, tree.path(), tree.path());
    wait_for_folder(&view, cx, tree.path()).await;
    let windows = cx.windows().len();
    let got = Rc::new(RefCell::new(None));
    let store = Rc::clone(&got);
    let nav = view.read_with(cx, |view, _| view.host().nav.clone());

    cx.update(|_, cx| {
        nav.update(cx, |nav, cx| {
            nav.pick_folder(
                "Add a folder",
                "Add",
                move |path, _| {
                    store.replace(Some(path));
                },
                cx,
            );
        });
    });
    let projects = tree.path().join("Projects");
    answer_picker(cx, windows, &projects);
    cx.run_until_parked();

    assert_eq!(got.borrow().as_deref(), Some(projects.as_path()));
}

fn answer_picker(cx: &mut VisualTestContext, windows: usize, folder: &Path) {
    cx.run_until_parked();
    let picker = cx
        .windows()
        .into_iter()
        .filter_map(|handle| handle.downcast::<MainWindow>())
        .find(|handle| {
            handle
                .read_with(cx, |view, cx| view.pick_id(cx).is_some())
                .unwrap()
        })
        .unwrap();
    assert_eq!(cx.windows().len(), windows + 1);
    picker
        .update(cx, |view, _, cx| {
            assert!(view.answer(folder.to_path_buf(), cx));
        })
        .unwrap();
}

pub fn right_click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let position = cx.debug_bounds(selector).unwrap().center();
    right_click_at(cx, position);
}

pub fn right_click_at(cx: &mut VisualTestContext, position: Point<Pixels>) {
    cx.run_until_parked();
    cx.simulate_event(MouseDownEvent {
        position,
        modifiers: Modifiers::none(),
        button: MouseButton::Right,
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position,
        modifiers: Modifiers::none(),
        button: MouseButton::Right,
        click_count: 1,
    });
    cx.run_until_parked();
}

#[gpui::test]
async fn the_tree_opens_in_place_and_follows_the_folder_on_screen(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("sidebar-tree");
    fs::create_dir(tree.path().join(".hidden")).unwrap();
    let (view, cx) = window_on(cx, tree.path(), tree.path());
    wait_for_folder(&view, cx, tree.path()).await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("tree-0").is_some());
    assert!(cx.debug_bounds("tree-1").is_none());

    click(cx, "tree-0-expander");
    wait_for_tree(&view, cx).await;
    assert!(cx.debug_bounds("tree-1").is_some());
    assert!(cx.debug_bounds("tree-2").is_none());

    view.update(cx, |view, _| view.open(&tree.path().join("Projects/nova")));
    wait_for_folder(&view, cx, &tree.path().join("Projects/nova")).await;
    wait_for_tree(&view, cx).await;
    assert!(cx.debug_bounds("tree-2").is_some());
    assert!(cx.debug_bounds("tree-1-expander").is_some());
    assert!(cx.debug_bounds("tree-2-expander").is_none());
    let from_row = |cx: &mut VisualTestContext, row: &'static str, part: &'static str| {
        cx.debug_bounds(part).unwrap().left() - cx.debug_bounds(row).unwrap().left()
    };
    assert_eq!(from_row(cx, "tree-0", "tree-0-expander"), px(2.0));
    assert_eq!(from_row(cx, "tree-1", "tree-1-expander"), px(24.0));
    assert_eq!(from_row(cx, "tree-2", "tree-2-guide-1"), px(18.0));
    assert_eq!(from_row(cx, "tree-2", "tree-2-guide-2"), px(40.0));

    click(cx, "tree-1-expander");
    cx.run_until_parked();
    assert!(cx.debug_bounds("tree-2").is_none());
    view.update(cx, |view, _| view.sort_by(nova_files_api::SortKey::Name));
    cx.run_until_parked();
    assert!(cx.debug_bounds("tree-2").is_none());
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.folder(),
            Some(tree.path().join("Projects/nova").as_path())
        );
    });

    click(cx, "tree-0");
    wait_for_folder(&view, cx, tree.path()).await;
}

#[gpui::test]
async fn the_system_drive_shows_its_free_space_and_opens(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("sidebar-drives");
    let (view, cx) = open_on(cx, tree.path()).await;
    view.condition::<WindowEvent>(cx, |view, cx| disk_space_known(view, Path::new("/"), cx))
        .await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("drive-0-number").is_some());
    cx.simulate_resize(size(NARROWEST_WIDE - px(1.0), px(600.0)));
    cx.run_until_parked();
    assert!(cx.debug_bounds("drive-0").is_some());
    assert!(cx.debug_bounds("drive-0-number").is_none());
    cx.simulate_resize(size(NARROWEST_WIDE, px(600.0)));
    cx.run_until_parked();

    right_click(cx, "drive-0");
    view.read_with(cx, |view, _| {
        let labels: Vec<_> = view
            .menu()
            .unwrap()
            .items
            .iter()
            .map(|item| item.label.clone())
            .collect();
        assert_eq!(labels, ["Open", "Open in new tab"]);
    });
    click(cx, "menu-0");
    wait_for_folder(&view, cx, Path::new("/")).await;

    right_click(cx, "drive-0");
    click(cx, "menu-1");
    wait_for_folder(&view, cx, Path::new("/")).await;
    view.read_with(cx, |view, _| {
        assert_eq!(view.tab_count(), 2);
        assert_eq!(view.current_tab(), 1);
    });
}
