use super::*;

const ALL: [&str; 5] = [
    "Projects",
    "banana.txt",
    "blue.txt",
    "my notes.txt",
    "notes.txt",
];

async fn named<'a>(
    cx: &'a mut TestAppContext,
    tree: &TempFolder,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    for name in ["banana.txt", "blue.txt", "my notes.txt"] {
        fs::write(tree.path().join(name), "").unwrap();
    }
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx) = open_on(cx, tree.path()).await;
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert_eq!(view.entry_names(), ALL));
    (view, cx)
}

async fn shows(view: &Entity<MainWindow>, cx: &VisualTestContext, names: &[&str]) {
    view.condition::<WindowEvent>(cx, |view, _| !view.waiting() && view.entry_names() == names)
        .await;
}

fn selected(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, _| view.selected_names())
}

fn filter(view: &Entity<MainWindow>, cx: &VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.filter_text().to_owned())
}

#[gpui::test]
async fn letters_narrow_the_list_and_pick_the_first_match(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("filter");
    let (view, cx) = named(cx, &tree).await;

    cx.simulate_keystrokes("n o t");
    shows(&view, cx, &["my notes.txt", "notes.txt"]).await;
    assert_eq!(selected(&view, cx), ["my notes.txt"]);
    assert_eq!(filter(&view, cx), "not");
    cx.run_until_parked();
    assert!(cx.debug_bounds("filter").is_some());
    assert!(cx.debug_bounds("count").is_none());
}

#[gpui::test]
async fn every_word_must_show_somewhere_in_the_name(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("filter");
    let (view, cx) = named(cx, &tree).await;

    cx.simulate_keystrokes("T X T space M Y");
    shows(&view, cx, &["my notes.txt"]).await;
    cx.simulate_keystrokes("z");
    shows(&view, cx, &[]).await;
    assert_eq!(selected(&view, cx), Vec::<String>::new());
}

#[gpui::test]
async fn escape_or_the_clear_button_brings_every_name_back(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("filter");
    let (view, cx) = named(cx, &tree).await;

    cx.simulate_keystrokes("b");
    shows(&view, cx, &["banana.txt", "blue.txt"]).await;
    cx.simulate_keystrokes("escape");
    shows(&view, cx, &ALL).await;
    assert_eq!(filter(&view, cx), "");
    assert_eq!(selected(&view, cx), ["banana.txt"]);

    cx.simulate_keystrokes("b l");
    shows(&view, cx, &["blue.txt"]).await;
    click(cx, "filter-clear");
    shows(&view, cx, &ALL).await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("filter").is_none());
}

#[gpui::test]
async fn backspace_takes_off_one_letter_at_a_time(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("filter");
    let (view, cx) = named(cx, &tree).await;

    cx.simulate_keystrokes("b l");
    shows(&view, cx, &["blue.txt"]).await;
    cx.simulate_keystrokes("backspace");
    shows(&view, cx, &["banana.txt", "blue.txt"]).await;
    cx.simulate_keystrokes("backspace");
    shows(&view, cx, &ALL).await;
    assert_eq!(filter(&view, cx), "");
}

#[gpui::test]
async fn keys_held_with_a_letter_and_a_leading_space_start_no_filter(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("filter");
    let (view, cx) = named(cx, &tree).await;

    for held in ["ctrl-b->b", "alt-b->b", "super-b->b", "fn-b->b", "tab"] {
        cx.simulate_keystrokes(held);
        assert_eq!(filter(&view, cx), "", "{held}");
    }
    cx.simulate_keystrokes("space");
    assert_eq!(filter(&view, cx), "");
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert_eq!(view.entry_names(), ALL));
}

#[gpui::test]
async fn opening_a_folder_drops_the_filter(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("filter");
    let (view, cx) = named(cx, &tree).await;

    cx.simulate_keystrokes("p r o");
    shows(&view, cx, &["Projects"]).await;
    cx.simulate_keystrokes("enter");
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;
    assert_eq!(filter(&view, cx), "");
    shows(&view, cx, &["nova"]).await;
    cx.simulate_keystrokes("backspace");
    wait_for_folder(&view, cx, tree.path()).await;
    shows(&view, cx, &ALL).await;
}
