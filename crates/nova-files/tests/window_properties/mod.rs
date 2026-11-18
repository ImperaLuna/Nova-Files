use std::os::unix::fs::MetadataExt as _;
use std::sync::Arc;

use nova_files_api::{PathKind, Properties, ScanState, Size};
use nova_files_ui::Icon;

mod access;

use super::*;
use crate::window_sidebar::right_click;

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
        let top = view.properties_top().unwrap();
        assert_eq!(top.amount.as_deref(), Some("12 B"));
        assert!(!top.has_bar);
        assert_eq!(top.share, None);
    });
    let folder_index: usize = folder_crumb["crumb-".len()..].parse().unwrap();
    assert_eq!(last_crumb(cx), format!("crumb-{}", folder_index + 1));

    let crumb = last_crumb(cx);
    click(cx, crumb);
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.location().cloned()),
        Some(Location::Properties(vec![notes.clone()]))
    );

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
        let (name, line) = view.properties_titles().unwrap();
        assert_eq!(name, "Projects");
        assert!(line.starts_with('~'), "{line}");
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

    fs::write(&notes, "back").unwrap();
    view.update(cx, MainWindow::go_back);
    view.condition::<WindowEvent>(cx, |view, _| {
        view.properties_page()
            .is_some_and(|page| page.read().first().is_some_and(|one| one.bytes == 4))
    })
    .await;
}

const CRUMBS_AT_MOST: usize = 64;

fn last_crumb(cx: &mut VisualTestContext) -> &'static str {
    let drawn = |n: usize| -> &'static str { Box::leak(format!("crumb-{n}").into_boxed_str()) };
    (0..CRUMBS_AT_MOST)
        .map(drawn)
        .take_while(|selector| cx.debug_bounds(selector).is_some())
        .last()
        .unwrap()
}

#[gpui::test]
async fn one_of_two_coming_back_keeps_the_page(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("properties-one-back");
    let notes = tree.path().join("notes.txt");
    let (view, cx) = properties_window(cx, tree.path()).await;
    click(cx, "name-0");
    click_with(cx, "name-1", Modifiers::secondary_key());
    view.update(cx, MainWindow::show_properties);
    read_page(&view, cx).await;

    let shown_again = |view: &Entity<MainWindow>, cx: &mut VisualTestContext| {
        view.update(cx, |view, cx| view.switch_tab(0, cx));
        view.update(cx, |view, cx| view.switch_tab(1, cx));
    };
    fs::remove_file(&notes).unwrap();
    shown_again(&view, cx);
    view.condition::<WindowEvent>(cx, |view, _| {
        view.properties_page()
            .is_some_and(|page| page.read().len() == 1)
    })
    .await;
    fs::write(&notes, "").unwrap();
    shown_again(&view, cx);
    view.condition::<WindowEvent>(cx, |view, _| {
        view.properties_page()
            .is_some_and(|page| page.read().len() == 2)
    })
    .await;
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.properties_page().is_some()));
}

#[gpui::test]
async fn a_search_ends_when_the_page_shows_again(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("properties-search");
    let (view, cx) = properties_window(cx, tree.path()).await;
    click(cx, "name-1");
    view.update(cx, MainWindow::show_properties);
    read_page(&view, cx).await;
    click(cx, "up");
    wait_for_folder(&view, cx, tree.path()).await;
    cx.simulate_keystrokes("ctrl-f");
    cx.run_until_parked();
    cx.simulate_input("notes");
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.searching()));

    view.update(cx, MainWindow::go_back);
    cx.run_until_parked();
    view.read_with(cx, |view, cx| {
        assert!(view.properties_page().is_some());
        assert!(!view.searching());
        assert_eq!(view.query(cx), "");
    });
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

async fn measured(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Size {
    view.condition::<WindowEvent>(cx, |view, _| {
        view.measured()
            .is_some_and(|measure| measure.state == ScanState::Done)
    })
    .await;
    view.read_with(cx, |view, _| view.measured().unwrap().size)
}

#[gpui::test]
async fn a_folder_is_measured_while_its_page_is_on_screen(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("properties-measure");
    let nova = tree.path().join("Projects").join("nova");
    fs::write(nova.join("a.rs"), "x".repeat(1_000)).unwrap();
    fs::write(nova.join("b.rs"), "x".repeat(234)).unwrap();
    let (view, cx) = properties_window(cx, tree.path()).await;
    click(cx, "name-0");
    view.update(cx, MainWindow::show_properties);
    let size = measured(&view, cx).await;
    assert_eq!(size.bytes, 1_234);
    assert_eq!(size.files, 2);
    assert_eq!(size.folders, 2);
    let top = view.read_with(cx, |view, _| view.properties_top().unwrap());
    assert!(top.amount.is_some());
    assert!(top.has_bar);
    let share = top.share.unwrap();
    assert!(share > 0.0 && share < 1.0, "{share}");

    view.update(cx, |view, cx| view.switch_tab(0, cx));
    assert_eq!(view.read_with(cx, |view, _| view.measured()), None);
    fs::write(nova.join("c.rs"), "x").unwrap();
    view.update(cx, |view, cx| view.switch_tab(1, cx));
    let shown = view.read_with(cx, |view, _| view.measured().unwrap().shown());
    assert_eq!(shown.bytes, 1_234);
    assert_eq!(measured(&view, cx).await.bytes, 1_235);
    let shown = view.read_with(cx, |view, _| view.measured().unwrap().shown());
    assert_eq!(shown.bytes, 1_235);
}

#[gpui::test]
async fn several_entries_add_up_files_and_folders_alike(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("properties-measure-several");
    fs::write(tree.path().join("notes.txt"), "x".repeat(10)).unwrap();
    fs::write(tree.path().join("Projects/nova/a"), "x".repeat(5)).unwrap();
    let (view, cx) = properties_window(cx, tree.path()).await;
    cx.simulate_keystrokes("ctrl-a");
    view.update(cx, MainWindow::show_properties);
    let size = measured(&view, cx).await;
    assert_eq!(size.bytes, 15);
    assert_eq!(size.files, 2);
    let top = view.read_with(cx, |view, _| view.properties_top().unwrap());
    assert_eq!(top.amount.as_deref(), Some("15 B"));
    assert!(!top.has_bar);
}

#[gpui::test]
async fn an_answer_for_another_page_stays_off_this_one(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("properties-other-page");
    let projects = tree.path().join("Projects");
    let (view, cx) = properties_window(cx, tree.path()).await;
    click(cx, "name-1");
    view.update(cx, MainWindow::show_properties);
    let place = Location::Properties(vec![projects.clone()]);
    view.update(cx, |view, cx| view.new_tab(&place, cx));
    let read = read_page(&view, cx).await;
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].path, projects);
}

#[gpui::test]
async fn leaving_the_page_stops_its_measuring(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("properties-measure-stop");
    let (view, cx) = properties_window(cx, tree.path()).await;
    click(cx, "name-0");
    view.update(cx, MainWindow::show_properties);
    view.update(cx, |view, cx| view.switch_tab(0, cx));
    cx.run_until_parked();
    view.update(cx, |view, cx| view.switch_tab(1, cx));
    assert_eq!(
        view.read_with(cx, |view, _| view.measured().map(|measure| measure.state)),
        Some(ScanState::Counting)
    );
    measured(&view, cx).await;

    click(cx, "up");
    wait_for_folder(&view, cx, tree.path()).await;
    assert_eq!(view.read_with(cx, |view, _| view.measured()), None);
}
