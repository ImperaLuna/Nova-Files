use super::*;

fn look_alike_tree() -> TempFolder {
    let tree = picker_tree();
    fs::write(
        tree.path()
            .join(OsString::from_vec(b"caf\x80.txt".to_vec())),
        "",
    )
    .unwrap();
    fs::write(tree.path().join("caf\u{fffd}.txt"), "").unwrap();
    tree
}

#[gpui::test]
async fn a_save_keeps_a_suggested_name_that_is_not_utf8(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let suggested = OsString::from_vec(b"caf\xe9.txt".to_vec());
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save(suggested.clone())).await;
    click(cx, "pick-accept");
    assert_eq!(
        answer(&view, cx, &answers).await,
        Some(picked(tree.path().join(suggested)))
    );
}

#[gpui::test]
async fn a_suggested_name_is_not_mistaken_for_one_that_looks_the_same(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    fs::write(
        tree.path()
            .join(OsString::from_vec(b"caf\xff.txt".to_vec())),
        "",
    )
    .unwrap();
    let suggested = OsString::from_vec(b"caf\xe9.txt".to_vec());
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save(suggested.clone())).await;
    cx.simulate_keystrokes("enter");
    assert_eq!(
        answer(&view, cx, &answers).await,
        Some(picked(tree.path().join(suggested)))
    );
}

#[gpui::test]
async fn a_typed_name_means_the_file_with_those_bytes(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = look_alike_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FILES).await;
    cx.simulate_input("caf\u{fffd}.txt");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        answer(&view, cx, &answers).await,
        Some(picked(tree.path().join("caf\u{fffd}.txt")))
    );
}

#[gpui::test]
async fn a_typed_paths_file_name_keeps_its_spaces(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    fs::write(tree.path().join(" notes.txt"), "").unwrap();
    let projects = tree.path().join("Projects");
    let (view, cx, answers) = picker(cx, &tree, &projects, FILES).await;
    cx.simulate_input("../ notes.txt");
    cx.simulate_keystrokes("enter");
    wait_for_name(&view, cx, " notes.txt").await;
    cx.simulate_keystrokes("enter");
    assert_eq!(
        answer(&view, cx, &answers).await,
        Some(picked(tree.path().join(" notes.txt")))
    );
}

#[gpui::test]
async fn retyping_a_clicked_name_means_what_was_typed(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = look_alike_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FILES).await;
    let first = view.read_with(cx, |view, _| {
        view.entry_names()
            .iter()
            .position(|name| name == "caf\u{fffd}.txt")
            .unwrap()
    });
    click(cx, format!("entry-{first}").leak());
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("caf\u{fffd}.txt");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        answer(&view, cx, &answers).await,
        Some(picked(tree.path().join("caf\u{fffd}.txt")))
    );
}

async fn saves_as_suggested(cx: &mut TestAppContext, suggested: &str) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save(suggested)).await;
    cx.simulate_keystrokes("enter");
    assert_eq!(
        answer(&view, cx, &answers).await,
        Some(picked(tree.path().join(suggested)))
    );
}

#[gpui::test]
async fn a_suggested_name_keeps_its_outer_spaces(cx: &mut TestAppContext) {
    saves_as_suggested(cx, " notes.txt").await;
}

#[gpui::test]
async fn a_suggested_tilde_is_a_name_not_home(cx: &mut TestAppContext) {
    saves_as_suggested(cx, "~").await;
}

#[gpui::test]
async fn a_suggested_path_is_read_as_typed_not_saved_blindly(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let projects = tree.path().join("Projects");
    let (view, cx, answers) = picker(cx, &tree, &projects, save("../notes.txt")).await;
    cx.simulate_keystrokes("enter");
    wait_for_folder(&view, cx, tree.path()).await;
    wait_for_name(&view, cx, "notes.txt").await;
    assert!(answers.try_recv().is_err());
    assert!(!closed(cx));
}

#[gpui::test]
async fn a_suggestion_typed_over_means_what_was_typed(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let suggested = OsString::from_vec(b"caf\x80.txt".to_vec());
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save(suggested)).await;
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("caf\u{fffd}.txt");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        answer(&view, cx, &answers).await,
        Some(picked(tree.path().join("caf\u{fffd}.txt")))
    );
}

#[gpui::test]
async fn a_space_typed_into_the_name_stays_in_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    fs::write(tree.path().join("a b.txt"), "").unwrap();
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx, answers) = picker(cx, &tree, tree.path(), FILES).await;
    cx.simulate_keystrokes("a space");
    cx.simulate_input("b.txt");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        answer(&view, cx, &answers).await,
        Some(picked(tree.path().join("a b.txt")))
    );
}

#[gpui::test]
async fn a_suggested_name_stays_in_the_box_in_another_folder(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    let projects = tree.path().join("Projects");
    let suggested = OsString::from_vec(b"caf\xe9.txt".to_vec());
    let (view, cx, answers) = picker(cx, &tree, tree.path(), save(suggested.clone())).await;
    click(cx, "entry-0");
    wait_for_folder(&view, cx, &projects).await;
    assert_eq!(
        name(&view, cx),
        Some(suggested.to_string_lossy().into_owned())
    );
    cx.simulate_keystrokes("enter");
    assert_eq!(
        answer(&view, cx, &answers).await,
        Some(picked(projects.join(suggested)))
    );
}

#[gpui::test]
async fn a_typed_name_stays_in_the_box_in_another_folder(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = picker_tree();
    cx.update(nova_files_window::bind_window_keys);
    let projects = tree.path().join("Projects");
    let (view, cx, answers) = picker(cx, &tree, &projects, save("a.txt")).await;
    cx.simulate_input("plan");
    cx.simulate_keystrokes("alt-up");
    wait_for_folder(&view, cx, tree.path()).await;
    assert_eq!(name(&view, cx).as_deref(), Some("plan.txt"));
    click(cx, "pick-accept");
    assert_eq!(
        answer(&view, cx, &answers).await,
        Some(picked(tree.path().join("plan.txt")))
    );
}
