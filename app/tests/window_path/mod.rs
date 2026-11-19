use gpui::{px, size};
use nova_files_ui::TextField;

use super::*;

async fn path_window<'a>(
    cx: &'a mut TestAppContext,
    tree: &TempFolder,
    folder: &Path,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    let (view, cx) = window_on(cx, folder, tree.path());
    wait_for_folder(&view, cx, folder).await;
    cx.run_until_parked();
    (view, cx)
}

fn drawn(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    cx.debug_bounds(selector).is_some()
}

#[gpui::test]
async fn each_crumb_goes_to_its_folder(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let nova = tree.path().join("Projects/nova");
    let (view, cx) = path_window(cx, &tree, &nova).await;

    assert!(drawn(cx, "crumb-0"));
    assert!(drawn(cx, "crumb-2"));
    assert!(!drawn(cx, "crumb-3"));
    assert!(drawn(cx, "arrow-0"));
    assert!(drawn(cx, "arrow-1"));
    assert!(!drawn(cx, "arrow-2"));

    click(cx, "crumb-1");
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;
    click(cx, "crumb-0");
    wait_for_folder(&view, cx, tree.path()).await;
}

async fn menu_names(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Vec<String> {
    view.condition::<WindowEvent>(cx, |view, cx| {
        view.path_menu_names(cx)
            .is_some_and(|names| !names.is_empty())
    })
    .await;
    cx.run_until_parked();
    view.read_with(cx, |view, cx| view.path_menu_names(cx).unwrap())
}

async fn menu_closed(view: &Entity<MainWindow>, cx: &mut VisualTestContext) {
    view.condition::<WindowEvent>(cx, |view, cx| view.path_menu_names(cx).is_none())
        .await;
    cx.run_until_parked();
    assert!(!drawn(cx, "path-menu"));
}

#[gpui::test]
async fn an_arrow_lists_the_folders_in_the_crumb_before_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    fs::create_dir(tree.path().join("Music")).unwrap();
    let nova = tree.path().join("Projects/nova");
    let (view, cx) = path_window(cx, &tree, &nova).await;

    click(cx, "arrow-0");
    assert_eq!(menu_names(&view, cx).await, ["Music", "Projects"]);
    assert!(drawn(cx, "path-menu-1"));
    click(cx, "path-menu-0");
    wait_for_folder(&view, cx, &tree.path().join("Music")).await;
    menu_closed(&view, cx).await;
}

#[gpui::test]
async fn a_press_on_an_open_arrow_closes_its_menu(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let nova = tree.path().join("Projects/nova");
    let (view, cx) = path_window(cx, &tree, &nova).await;

    click(cx, "arrow-1");
    assert_eq!(menu_names(&view, cx).await, ["nova"]);
    click(cx, "arrow-1");
    menu_closed(&view, cx).await;
    click(cx, "arrow-1");
    menu_names(&view, cx).await;
    click(cx, "arrow-0");
    assert_eq!(menu_names(&view, cx).await, ["Projects"]);
}

#[gpui::test]
async fn going_elsewhere_closes_a_menu_for_good(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let nova = tree.path().join("Projects/nova");
    let (view, cx) = path_window(cx, &tree, &nova).await;

    click(cx, "arrow-1");
    assert_eq!(menu_names(&view, cx).await, ["nova"]);
    view.update(cx, |view, _| view.go_up());
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;
    view.update(cx, MainWindow::go_back);
    wait_for_folder(&view, cx, &nova).await;
    menu_closed(&view, cx).await;
}

#[gpui::test]
async fn the_folder_on_screen_has_an_arrow_only_when_it_holds_folders(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let projects = tree.path().join("Projects");
    let (view, cx) = path_window(cx, &tree, &projects).await;

    assert!(drawn(cx, "arrow-1"));
    click(cx, "arrow-1");
    assert_eq!(menu_names(&view, cx).await, ["nova"]);
    click(cx, "path-menu-0");
    wait_for_folder(&view, cx, &projects.join("nova")).await;
    cx.run_until_parked();
    assert!(!drawn(cx, "arrow-2"));
}

#[gpui::test]
async fn a_menu_whose_folder_is_gone_or_holds_no_folders_closes(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let deep = tree.path().join("a/b/c");
    fs::create_dir_all(&deep).unwrap();
    let (view, cx) = path_window(cx, &tree, &deep).await;

    fs::remove_dir(&deep).unwrap();
    click(cx, "arrow-2");
    menu_closed(&view, cx).await;
    fs::remove_dir_all(tree.path().join("a")).unwrap();
    click(cx, "arrow-1");
    menu_closed(&view, cx).await;
}

#[gpui::test]
async fn a_long_path_folds_its_middle_into_a_menu(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let deep = tree.path().join("a/b/c/d/e/f");
    fs::create_dir_all(&deep).unwrap();
    let (view, cx) = path_window(cx, &tree, &deep).await;

    for shown in ["crumb-0", "crumb-more", "crumb-4", "crumb-5", "crumb-6"] {
        assert!(drawn(cx, shown), "{shown} not drawn");
    }
    for folded in ["crumb-1", "crumb-2", "crumb-3", "arrow-1", "arrow-3"] {
        assert!(!drawn(cx, folded), "{folded} drawn");
    }

    click(cx, "crumb-more");
    assert_eq!(menu_names(&view, cx).await, ["a", "b", "c"]);
    assert!(drawn(cx, "path-menu"));
    click(cx, "path-menu-1");
    wait_for_folder(&view, cx, &tree.path().join("a/b")).await;
    menu_closed(&view, cx).await;
}

#[gpui::test]
async fn a_click_on_the_folded_crumb_again_closes_its_menu(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let deep = tree.path().join("a/b/c/d/e/f");
    fs::create_dir_all(&deep).unwrap();
    let (view, cx) = path_window(cx, &tree, &deep).await;

    click(cx, "crumb-more");
    menu_names(&view, cx).await;
    click(cx, "crumb-more");
    menu_closed(&view, cx).await;
}

#[gpui::test]
async fn a_narrow_window_keeps_the_folder_on_screen_in_view(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let deep = tree
        .path()
        .join("a long folder name/another long folder name/the folder");
    fs::create_dir_all(&deep).unwrap();
    let (_view, cx) = path_window(cx, &tree, &deep).await;

    cx.simulate_resize(size(px(560.0), px(400.0)));
    cx.run_until_parked();
    let path = cx.debug_bounds("path").unwrap();
    let here = cx.debug_bounds("crumb-3").unwrap();
    let home = cx.debug_bounds("crumb-0").unwrap();
    assert!(here.left() >= path.left() && here.right() <= path.right());
    assert!(home.left() < path.left());
}

async fn typing_window<'a>(
    cx: &'a mut TestAppContext,
    tree: &TempFolder,
    folder: &Path,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        nova_files_ui::bind_window_keys(cx);
        nova_files_ui::bind_text_keys(cx);
    });
    path_window(cx, tree, folder).await
}

fn path_field(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Option<Entity<TextField>> {
    view.read_with(cx, MainWindow::path_field)
}

fn typed(view: &Entity<MainWindow>, cx: &VisualTestContext) -> (String, std::ops::Range<usize>) {
    let field = path_field(view, cx).unwrap();
    field.read_with(cx, |field, _| (field.text().to_owned(), field.selection()))
}

#[gpui::test]
async fn a_click_on_the_empty_bar_types_a_path_from_the_folder_on_screen(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let projects = tree.path().join("Projects");
    let (view, cx) = typing_window(cx, &tree, &projects).await;

    click(cx, "path-fill");
    assert_eq!(typed(&view, cx), ("~/Projects".to_owned(), 0..10));
    assert!(!drawn(cx, "crumb-0"));
    cx.simulate_input("nova");
    cx.simulate_keystrokes("enter");
    wait_for_folder(&view, cx, &projects.join("nova")).await;
    cx.run_until_parked();
    assert!(path_field(&view, cx).is_none());
    assert!(drawn(cx, "crumb-2"));
}

#[gpui::test]
async fn a_typed_tilde_or_full_path_goes_there(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let projects = tree.path().join("Projects");
    let (view, cx) = typing_window(cx, &tree, &projects).await;

    click(cx, "path-fill");
    cx.simulate_input("~");
    cx.simulate_keystrokes("enter");
    wait_for_folder(&view, cx, tree.path()).await;

    click(cx, "path-fill");
    let nova = projects.join("nova");
    cx.simulate_input(nova.to_str().unwrap());
    cx.simulate_keystrokes("enter");
    wait_for_folder(&view, cx, &nova).await;
}

#[gpui::test]
async fn a_folder_that_cant_be_read_keeps_the_text_selected(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let (view, cx) = typing_window(cx, &tree, tree.path()).await;

    click(cx, "path-fill");
    cx.simulate_input("missing");
    cx.simulate_keystrokes("enter");
    view.condition::<WindowEvent>(cx, |view, _| !view.waiting())
        .await;
    cx.run_until_parked();
    assert_eq!(typed(&view, cx), ("missing".to_owned(), 0..7));
    assert_eq!(
        view.read_with(cx, |view, _| view.folder().map(Path::to_path_buf)),
        Some(tree.path().to_path_buf())
    );
}

#[gpui::test]
async fn escape_or_enter_on_the_same_path_puts_the_crumbs_back(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let projects = tree.path().join("Projects");
    let (view, cx) = typing_window(cx, &tree, &projects).await;

    click(cx, "path-fill");
    cx.simulate_input("nova");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(path_field(&view, cx).is_none());
    cx.simulate_keystrokes("alt-up");
    wait_for_folder(&view, cx, tree.path()).await;

    click(cx, "path-fill");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(path_field(&view, cx).is_none());
    assert!(!view.read_with(cx, |view, _| view.waiting()));
}

#[gpui::test]
async fn typing_the_folder_on_screen_reads_it_again_and_puts_the_crumbs_back(
    cx: &mut TestAppContext,
) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let (view, cx) = typing_window(cx, &tree, tree.path()).await;

    click(cx, "path-fill");
    cx.simulate_input(".");
    cx.simulate_keystrokes("enter");
    view.condition::<WindowEvent>(cx, |view, cx| view.path_field(cx).is_none())
        .await;
}

#[gpui::test]
async fn going_elsewhere_or_clicking_away_puts_the_crumbs_back(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let projects = tree.path().join("Projects");
    let (view, cx) = typing_window(cx, &tree, &projects).await;

    click(cx, "path-fill");
    cx.simulate_keystrokes("alt-up");
    wait_for_folder(&view, cx, tree.path()).await;
    cx.run_until_parked();
    assert!(path_field(&view, cx).is_none());

    click(cx, "path-fill");
    click(cx, "entry-1");
    cx.run_until_parked();
    assert!(path_field(&view, cx).is_none());
}

#[gpui::test]
async fn a_space_typed_into_the_path_stays_in_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("path");
    let projects = tree.path().join("Projects");
    let (view, cx) = typing_window(cx, &tree, &projects).await;

    click(cx, "path-fill");
    cx.simulate_input("a");
    cx.simulate_keystrokes("space");
    cx.run_until_parked();
    assert_eq!(typed(&view, cx).0, "a ");
    assert!(!view.read_with(cx, |view, _| view.previewing()));
}
