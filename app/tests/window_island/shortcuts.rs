use nova_files_window::Group;

use super::*;

fn marked(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Option<&'static str> {
    view.read_with(cx, |view, _| {
        view.shortcuts()?.picking.marked_spec().map(|spec| spec.id)
    })
}

fn runs(view: &Entity<MainWindow>, cx: &VisualTestContext, id: &str) -> bool {
    view.read_with(cx, |view, _| {
        view.shortcuts()
            .unwrap()
            .picking
            .list
            .iter()
            .find(|shortcut| shortcut.spec.id == id)
            .unwrap()
            .runs
    })
}

fn open(view: &Entity<MainWindow>, cx: &VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.shortcuts().is_some())
}

#[gpui::test]
async fn question_mark_opens_the_sheet_and_enter_runs_what_was_typed(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("shortcuts");
    let home = TempFolder::tree("shortcuts-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    cx.simulate_keystrokes("?");
    cx.run_until_parked();
    assert!(open(&view, cx));
    assert!(cx.debug_bounds("shortcuts").is_some());
    cx.simulate_input("select all");
    cx.run_until_parked();
    assert_eq!(marked(&view, cx), Some("select.all"));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(!open(&view, cx));
    assert_eq!(view.read_with(cx, |view, _| view.selected_count()), 2);
}

#[gpui::test]
async fn a_key_typed_in_full_finds_its_command(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("shortcuts");
    let home = TempFolder::tree("shortcuts-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    cx.simulate_keystrokes("ctrl-shift-p");
    cx.run_until_parked();
    cx.simulate_input("ctrl a");
    cx.run_until_parked();
    assert_eq!(marked(&view, cx), Some("select.all"));
}

#[gpui::test]
async fn escape_or_the_same_key_closes_it_and_the_list_has_the_keys_again(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("shortcuts");
    let home = TempFolder::tree("shortcuts-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    cx.simulate_keystrokes("?");
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!open(&view, cx));
    cx.simulate_keystrokes("ctrl-shift-p");
    cx.run_until_parked();
    assert!(open(&view, cx));
    cx.simulate_keystrokes("ctrl-shift-p");
    cx.run_until_parked();
    assert!(!open(&view, cx));
    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.selected_count()), 1);
}

#[gpui::test]
async fn arrows_move_the_mark_and_skip_what_cannot_run(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("shortcuts");
    let home = TempFolder::tree("shortcuts-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    cx.simulate_keystrokes("?");
    cx.run_until_parked();
    assert!(!runs(&view, cx, "file.compress"));
    assert!(runs(&view, cx, "select.all"));
    cx.simulate_input("compress");
    cx.run_until_parked();
    assert_eq!(marked(&view, cx), None);
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    assert!(!open(&view, cx));

    click(cx, "name-1");
    cx.simulate_keystrokes("?");
    cx.run_until_parked();
    assert!(runs(&view, cx, "file.compress"));
    let first = marked(&view, cx);
    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    assert_ne!(marked(&view, cx), first);
    cx.simulate_keystrokes("up");
    cx.run_until_parked();
    assert_eq!(marked(&view, cx), first);
    assert_eq!(view.read_with(cx, |view, _| view.selected_count()), 1);
}

#[gpui::test]
async fn a_click_on_a_row_runs_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("shortcuts");
    let home = TempFolder::tree("shortcuts-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    cx.simulate_keystrokes("?");
    cx.run_until_parked();
    cx.simulate_input("select all");
    cx.run_until_parked();
    click(cx, "shortcut-0");
    cx.run_until_parked();
    assert!(!open(&view, cx));
    assert_eq!(view.read_with(cx, |view, _| view.selected_count()), 2);
}

#[gpui::test]
async fn question_mark_in_a_name_field_is_typed(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("shortcuts");
    let home = TempFolder::tree("shortcuts-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    click(cx, "name-1");
    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    cx.simulate_keystrokes("? space");
    cx.run_until_parked();
    assert!(!open(&view, cx));
    let typed = view.read_with(cx, |view, cx| {
        view.rename_field(1, cx).unwrap().read(cx).text().to_owned()
    });
    assert_eq!(typed, "? .txt");
}

#[gpui::test]
async fn every_command_has_a_group_of_its_own(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("shortcuts");
    let home = TempFolder::tree("shortcuts-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;

    cx.simulate_keystrokes("?");
    cx.run_until_parked();
    let homeless: Vec<&str> = view.read_with(cx, |view, _| {
        let list = &view.shortcuts().unwrap().picking.list;
        list.iter()
            .filter(|shortcut| shortcut.group == Group::More)
            .map(|shortcut| shortcut.spec.id)
            .collect()
    });
    assert_eq!(homeless, Vec::<&str>::new());
}
