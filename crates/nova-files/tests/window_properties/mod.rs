//! The Properties page on a real core: the right-click menu and Alt+Enter
//! open it in a new tab, it shows what the disk says, Up goes to the
//! folder that holds what it shows, a tab shown again reads it again,
//! and a page whose files are all gone goes to Start.

use std::os::unix::fs::MetadataExt as _;
use std::sync::Arc;

use nova_files_api::{PathKind, Properties};
use nova_files_ui::Icon;

use super::*;
use crate::window_sidebar::right_click;

/// A window on `folder` with its keys bound, the folder on screen.
async fn properties_window<'a>(
    cx: &'a mut TestAppContext,
    folder: &Path,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        nova_files_ui::bind_window_keys(cx);
        nova_files_ui::bind_text_keys(cx);
    });
    open_on(cx, folder).await
}

/// Waits until the Properties page on screen has every answer in, and
/// returns them.
async fn read_page(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Vec<Arc<Properties>> {
    view.condition::<WindowEvent>(cx, |view, _| {
        view.properties_page()
            .is_some_and(nova_files_ui::PropertiesPage::complete)
    })
    .await;
    cx.run_until_parked();
    view.read_with(cx, |view, _| view.properties_page().unwrap().read())
}

#[gpui::test]
async fn the_menu_opens_a_files_properties_in_a_new_tab(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("properties-menu");
    let notes = tree.path().join("notes.txt");
    fs::write(&notes, "twelve bytes").unwrap();
    let (view, cx) = properties_window(cx, tree.path()).await;

    let folder_crumb = last_crumb(cx);
    right_click(cx, "name-1");
    let last = view.read_with(cx, |view, _| {
        let menu = view.menu().unwrap();
        assert_eq!(menu.items.last().unwrap().label, "Properties");
        menu.items.len() - 1
    });
    let selector: &'static str = ["menu-5", "menu-6", "menu-7"][last - 5];
    click(cx, selector);

    let read = read_page(&view, cx).await;
    let disk = fs::symlink_metadata(&notes).unwrap();
    let [one] = &read[..] else {
        panic!("one item, got {read:?}")
    };
    assert_eq!(one.path, notes);
    assert_eq!(one.kind, PathKind::File);
    assert_eq!(one.bytes, disk.len());
    assert_eq!(one.mode, disk.mode() & 0o7777);
    assert_eq!(one.modified, disk.modified().ok());
    view.read_with(cx, |view, _| {
        assert_eq!(view.tab_count(), 2);
        assert_eq!(view.current_tab(), 1);
        assert_eq!(view.tab_names()[1], "notes.txt");
        assert_eq!(view.folder(), None);
        assert_eq!(view.parent().as_deref(), Some(tree.path()));
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("properties-top").is_some());
    assert!(cx.debug_bounds("properties-date-0").is_some());
    view.read_with(cx, |view, _| {
        let (name, line) = view.properties_titles().unwrap();
        assert_eq!(name, "notes.txt");
        assert_eq!(line, "TXT file · ~");
        assert_eq!(view.properties_icon(), Some(Icon::File));
    });
    // The path bar goes one crumb past the folder, to the file.
    let folder_index: usize = folder_crumb["crumb-".len()..].parse().unwrap();
    assert_eq!(last_crumb(cx), format!("crumb-{}", folder_index + 1));

    // The file's own crumb is the page: a click there stays.
    let crumb = last_crumb(cx);
    click(cx, crumb);
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.location().cloned()),
        Some(Location::Properties(vec![notes.clone()]))
    );

    // Up goes to the folder that holds it.
    click(cx, "up");
    wait_for_folder(&view, cx, tree.path()).await;
    view.update(cx, MainWindow::go_back);
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.location().cloned()),
        Some(Location::Properties(vec![notes]))
    );
}

#[gpui::test]
async fn a_lone_folder_keeps_its_own_icon(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("properties-folder");
    let (view, cx) = properties_window(cx, tree.path()).await;
    click(cx, "name-0");
    view.update(cx, MainWindow::show_properties);
    read_page(&view, cx).await;
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.properties_titles(),
            Some(("Projects".to_owned(), "~".to_owned()))
        );
        assert_eq!(view.properties_icon(), None);
    });
}

#[gpui::test]
async fn alt_enter_shows_several_entries_on_one_page(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("properties-several");
    fs::write(tree.path().join("todo.txt"), "").unwrap();
    let (view, cx) = properties_window(cx, tree.path()).await;
    click(cx, "name-0");
    click_with(cx, "name-1", Modifiers::secondary_key());
    click_with(cx, "name-2", Modifiers::secondary_key());

    cx.simulate_keystrokes("alt-enter");
    let read = read_page(&view, cx).await;
    let kinds: Vec<PathKind> = read.iter().map(|one| one.kind.clone()).collect();
    assert_eq!(kinds, [PathKind::Folder, PathKind::File, PathKind::File]);
    view.read_with(cx, |view, _| {
        assert_eq!(view.tab_names()[1], "3 items");
        let (name, line) = view.properties_titles().unwrap();
        assert_eq!(name, "3 items");
        assert_eq!(line, "1 folder, 2 files · ~");
        assert_eq!(view.properties_icon(), None);
        assert_eq!(view.parent().as_deref(), Some(tree.path()));
    });
}

#[gpui::test]
async fn a_page_shown_again_reads_again_and_goes_to_start_once_all_is_gone(
    cx: &mut TestAppContext,
) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("properties-again");
    let notes = tree.path().join("notes.txt");
    let (view, cx) = properties_window(cx, tree.path()).await;
    click(cx, "name-1");
    view.update(cx, MainWindow::show_properties);
    // Nothing shows until the core answers.
    view.read_with(cx, |view, _| {
        assert!(!view.properties_page().unwrap().complete());
        assert_eq!(view.properties_titles(), None);
    });
    assert_eq!(read_page(&view, cx).await[0].bytes, 0);

    view.update(cx, |view, cx| view.switch_tab(0, cx));
    fs::write(&notes, "more").unwrap();
    view.update(cx, |view, cx| view.switch_tab(1, cx));
    view.condition::<WindowEvent>(cx, |view, _| {
        view.properties_page()
            .is_some_and(|page| page.read().first().is_some_and(|one| one.bytes == 4))
    })
    .await;

    view.update(cx, |view, cx| view.switch_tab(0, cx));
    fs::remove_file(&notes).unwrap();
    view.update(cx, |view, cx| view.switch_tab(1, cx));
    view.condition::<WindowEvent>(cx, |view, _| view.is_on_start())
        .await;

    // Once it is back, Back shows it again.
    fs::write(&notes, "back").unwrap();
    view.update(cx, MainWindow::go_back);
    view.condition::<WindowEvent>(cx, |view, _| {
        view.properties_page()
            .is_some_and(|page| page.read().first().is_some_and(|one| one.bytes == 4))
    })
    .await;
}

/// More crumbs than any test's path has.
const CRUMBS_AT_MOST: usize = 64;

/// The last crumb drawn in the path bar.
fn last_crumb(cx: &mut VisualTestContext) -> &'static str {
    let drawn = |n: usize| -> &'static str { Box::leak(format!("crumb-{n}").into_boxed_str()) };
    (0..CRUMBS_AT_MOST)
        .map(drawn)
        .take_while(|selector| cx.debug_bounds(selector).is_some())
        .last()
        .unwrap()
}

#[gpui::test]
async fn the_trash_has_no_properties(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("properties-trash");
    let (view, cx) = properties_window(cx, tree.path()).await;
    click(cx, "name-1");
    view.update(cx, MainWindow::trash_selected);
    view.condition::<WindowEvent>(cx, |view, _| view.entry_names() == ["Projects"])
        .await;
    view.update(cx, MainWindow::open_trash_in_new_tab);
    view.condition::<WindowEvent>(cx, |view, _| view.entry_names() == ["notes.txt"])
        .await;
    click(cx, "name-0");
    assert_eq!(view.read_with(cx, |view, _| view.selected_count()), 1);

    cx.simulate_keystrokes("alt-enter");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.tab_count()), 2);
}
