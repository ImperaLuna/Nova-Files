use super::*;
use crate::window_sidebar::right_click;

fn arrows_move(view: &Entity<MainWindow>, cx: &mut VisualTestContext, after: &str) {
    cx.run_until_parked();
    let before = view.read_with(cx, |view, _| view.cursor());
    cx.simulate_keystrokes(if before == Some(0) { "down" } else { "up" });
    cx.run_until_parked();
    let now = view.read_with(cx, |view, _| view.cursor());
    assert!(
        now.is_some() && now != before,
        "the arrows did nothing after {after}: {before:?} {now:?} {:?}",
        view.read_with(cx, |view, _| view.selected_names())
    );
}

async fn many<'a>(
    cx: &'a mut TestAppContext,
    tree: &TempFolder,
    home: &TempFolder,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    for index in 0..20 {
        fs::write(tree.path().join(format!("file-{index:02}")), "").unwrap();
    }
    island_window(cx, tree.path(), home).await
}

#[gpui::test]
async fn the_arrows_move_the_ring_after_a_menu_a_rename_or_a_field_closes(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("focus-return");
    let home = TempFolder::tree("focus-return-home");
    let (view, cx) = many(cx, &tree, &home).await;

    click(cx, "name-1");
    right_click(cx, "entry-2");
    cx.simulate_keystrokes("escape");
    arrows_move(&view, cx, "Esc on a menu");

    cx.simulate_keystrokes("f2");
    cx.run_until_parked();
    cx.simulate_input("renamed");
    cx.simulate_keystrokes("enter");
    view.condition::<WindowEvent>(cx, |view, _| {
        view.entry_names().iter().any(|name| name == "renamed")
    })
    .await;
    arrows_move(&view, cx, "a rename");

    cx.simulate_keystrokes("ctrl-l");
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    arrows_move(&view, cx, "Esc in the path bar");

    cx.simulate_keystrokes("ctrl-f");
    cx.run_until_parked();
    cx.simulate_input("file");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    arrows_move(&view, cx, "Esc in search");
}

#[gpui::test]
async fn the_arrows_move_the_ring_after_a_click_on_the_window_around_the_list(
    cx: &mut TestAppContext,
) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("focus-click");
    let home = TempFolder::tree("focus-click-home");
    let (view, cx) = many(cx, &tree, &home).await;

    click(cx, "name-1");
    click(cx, "head-name");
    arrows_move(&view, cx, "a column head");

    click(cx, "island-copy");
    arrows_move(&view, cx, "an island button");

    view.update(cx, |view, cx| {
        view.new_tab(&Location::start(), cx);
    });
    cx.run_until_parked();
    click(cx, "tab-0");
    wait_for_folder(&view, cx, tree.path()).await;
    arrows_move(&view, cx, "a tab");

    click(cx, "place-0");
    cx.run_until_parked();
    assert_ne!(
        view.read_with(cx, |view, _| view.folder().map(Path::to_path_buf)),
        Some(tree.path().to_path_buf())
    );
    arrows_move(&view, cx, "a Place");
}
