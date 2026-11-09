//! Type to jump in a main window on a real core: letters move the cursor
//! to the next name starting with them, a repeated letter steps through
//! its names, and a pause starts a new prefix.

use std::time::Duration;

use super::*;

/// The shared tree plus `banana.txt`, `blue.txt` and `my notes.txt`:
/// Projects, banana.txt, blue.txt, my notes.txt, notes.txt.
async fn named<'a>(
    cx: &'a mut TestAppContext,
    tree: &TempFolder,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    for name in ["banana.txt", "blue.txt", "my notes.txt"] {
        fs::write(tree.path().join(name), "").unwrap();
    }
    cx.update(nova_files_ui::bind_window_keys);
    let (view, cx) = open_on(cx, tree.path()).await;
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.entry_names(),
            [
                "Projects",
                "banana.txt",
                "blue.txt",
                "my notes.txt",
                "notes.txt"
            ]
        );
    });
    (view, cx)
}

fn selected(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, _| view.selected_names())
}

/// Longer than the pause that starts a new prefix.
const PAUSED: Duration = Duration::from_secs(2);

#[gpui::test]
async fn letters_jump_to_a_name_and_build_a_prefix(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("jump");
    let (view, cx) = named(cx, &tree).await;

    cx.simulate_keystrokes("n");
    assert_eq!(selected(&view, cx), ["notes.txt"]);
    cx.executor().advance_clock(PAUSED);
    cx.simulate_keystrokes("b l");
    assert_eq!(selected(&view, cx), ["blue.txt"]);
    cx.executor().advance_clock(PAUSED);
    // A space inside a name is part of it.
    cx.simulate_keystrokes("m y space n");
    assert_eq!(selected(&view, cx), ["my notes.txt"]);
}

#[gpui::test]
async fn a_repeated_letter_steps_through_its_names(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("jump");
    let (view, cx) = named(cx, &tree).await;

    cx.simulate_keystrokes("b");
    assert_eq!(selected(&view, cx), ["banana.txt"]);
    cx.simulate_keystrokes("b");
    assert_eq!(selected(&view, cx), ["blue.txt"]);
    cx.simulate_keystrokes("b");
    assert_eq!(selected(&view, cx), ["banana.txt"]);
}

#[gpui::test]
async fn a_name_nothing_starts_with_leaves_the_cursor(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("jump");
    let (view, cx) = named(cx, &tree).await;

    cx.simulate_keystrokes("b z");
    assert_eq!(selected(&view, cx), ["banana.txt"]);
    // A space with nothing typed before it isn't a jump.
    cx.executor().advance_clock(PAUSED);
    cx.simulate_keystrokes("space");
    assert_eq!(selected(&view, cx), ["banana.txt"]);
}

#[gpui::test]
async fn keys_held_with_a_letter_are_not_a_jump(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("jump");
    let (view, cx) = named(cx, &tree).await;

    for held in ["ctrl-b->b", "alt-b->b", "super-b->b", "fn-b->b"] {
        cx.simulate_keystrokes(held);
        assert_eq!(selected(&view, cx), Vec::<String>::new(), "{held}");
    }
}

#[gpui::test]
async fn a_tab_or_a_leading_space_is_not_part_of_a_name(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("jump");
    let (view, cx) = named(cx, &tree).await;

    cx.simulate_keystrokes("space n");
    assert_eq!(selected(&view, cx), ["notes.txt"]);
    cx.executor().advance_clock(PAUSED);
    cx.simulate_keystrokes("b tab l");
    assert_eq!(selected(&view, cx), ["blue.txt"]);
}

#[gpui::test]
async fn a_new_folder_starts_a_new_prefix(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("jump");
    let (view, cx) = named(cx, &tree).await;

    cx.simulate_keystrokes("p enter");
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;
    cx.simulate_keystrokes("n");
    assert_eq!(selected(&view, cx), ["nova"]);
}
