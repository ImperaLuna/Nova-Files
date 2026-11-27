use super::*;

#[gpui::test]
async fn the_picker_offers_the_gathered_places(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FOLDERS).await;
    click(cx, "place-projects");
    view.condition::<WindowEvent>(cx, |view, _| view.entry_names() == ["nova"])
        .await;
    double_click(cx, "entry-0");
    wait_for_folder(&view, cx, &tree.path().join("Projects/nova")).await;
    assert!(answers.try_recv().is_err());
}

#[gpui::test]
async fn the_picker_scrolls_drives_with_the_places(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (_view, cx, _answers) = picker(cx, &tree, tree.path(), FILES).await;
    cx.run_until_parked();
    let scroll = cx.debug_bounds("sidebar-scroll").unwrap();
    let drives = cx.debug_bounds("drives").unwrap();
    assert!(drives.origin.y < scroll.bottom());
}
