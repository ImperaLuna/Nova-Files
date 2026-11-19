use super::*;

#[gpui::test]
async fn arrows_put_the_file_selected_in_the_box(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FILES).await;
    cx.simulate_input("typed");

    cx.simulate_keystrokes("down");
    assert_eq!(name(&view, cx).as_deref(), Some("typed"));
    cx.simulate_keystrokes("down down");
    assert_eq!(name(&view, cx).as_deref(), Some("notes.txt"));
    cx.simulate_keystrokes("enter");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked(tree.path().join("notes.txt")))
    );
}

#[gpui::test]
async fn enter_goes_into_a_folder_the_arrows_landed_on(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FILES).await;

    cx.simulate_keystrokes("down enter");
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;
    assert!(answers.try_recv().is_err());
}

#[gpui::test]
async fn a_picker_for_one_file_keeps_one_selected(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FILES).await;

    cx.simulate_keystrokes("down down shift-down");
    let selected = view.read_with(cx, |view, _| view.selected_names());
    assert_eq!(selected, ["notes.txt"]);
    assert_eq!(name(&view, cx).as_deref(), Some("notes.txt"));
    cx.simulate_keystrokes("enter");
    assert_eq!(
        answers.try_recv().ok(),
        Some(picked(tree.path().join("notes.txt")))
    );
}

#[gpui::test]
async fn a_picker_deletes_nothing_for_good(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx, _answers) = picker(cx, &tree, tree.path(), FILES).await;

    click(cx, "entry-2");
    cx.simulate_keystrokes("shift-delete");
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.confirm().is_none()));
    assert!(tree.path().join("notes.txt").exists());
}
