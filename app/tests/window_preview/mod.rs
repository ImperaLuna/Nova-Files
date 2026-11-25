use super::*;

async fn with_text<'a>(
    cx: &'a mut TestAppContext,
    tree: &TempFolder,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    fs::write(tree.path().join("a.txt"), "first line\nsecond line\n").unwrap();
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx) = open_on(cx, tree.path()).await;
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.entry_names(), ["Projects", "a.txt", "notes.txt"]);
    });
    (view, cx)
}

async fn wait_for_preview(view: &Entity<MainWindow>, cx: &VisualTestContext, name: &str) {
    view.condition::<WindowEvent>(cx, |view, cx| {
        previewed(view, cx).is_some_and(|shown| shown.name == name && shown.preview.is_some())
    })
    .await;
}

fn previewed_name(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Option<String> {
    view.read_with(cx, |view, cx| previewed(view, cx).map(|shown| shown.name))
}

fn selected(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, _| view.selected_names())
}

#[gpui::test]
async fn space_shows_the_selected_file_and_closes_it_again(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    cx.simulate_keystrokes("space");
    assert!(!view.read_with(cx, previewing));

    click(cx, "entry-1");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "a.txt").await;
    let preview = view.read_with(cx, |view, cx| previewed(view, cx).unwrap().preview.unwrap());
    assert_eq!(
        preview.content,
        nova_files_preview_api::PreviewContent::Text {
            text: "first line\nsecond line\n".into(),
            cut: false
        }
    );
    cx.run_until_parked();
    assert!(cx.debug_bounds("preview").is_some());

    cx.simulate_keystrokes("space");
    assert_eq!(previewed_name(&view, cx), None);
    cx.run_until_parked();
    assert!(cx.debug_bounds("preview").is_none());
    assert_eq!(selected(&view, cx), ["a.txt"]);
}

#[gpui::test]
async fn escape_closes_the_quick_look_and_keeps_the_selection(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    click(cx, "entry-1");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "a.txt").await;
    cx.simulate_keystrokes("escape");
    assert_eq!(previewed_name(&view, cx), None);
    assert_eq!(selected(&view, cx), ["a.txt"]);
}

#[gpui::test]
async fn the_arrow_keys_step_through_the_folder_and_the_selection_follows(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    click(cx, "entry-1");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "a.txt").await;

    cx.simulate_keystrokes("right");
    wait_for_preview(&view, cx, "notes.txt").await;
    assert_eq!(selected(&view, cx), ["notes.txt"]);
    cx.simulate_keystrokes("down");
    assert_eq!(previewed_name(&view, cx).as_deref(), Some("notes.txt"));

    cx.simulate_keystrokes("up left");
    wait_for_preview(&view, cx, "Projects").await;
    assert_eq!(selected(&view, cx), ["Projects"]);
    let preview = view.read_with(cx, |view, cx| previewed(view, cx).unwrap().preview.unwrap());
    match &preview.content {
        nova_files_preview_api::PreviewContent::Folder { first, count } => {
            assert_eq!(*count, 1);
            assert_eq!(first[0].name, "nova");
        }
        other => panic!("expected a folder, got {other:?}"),
    }
}

#[gpui::test]
async fn the_buttons_step_and_close(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    click(cx, "entry-1");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "a.txt").await;
    click(cx, "preview-next");
    wait_for_preview(&view, cx, "notes.txt").await;
    click(cx, "preview-previous");
    wait_for_preview(&view, cx, "a.txt").await;
    click(cx, "preview-art");
    assert!(view.read_with(cx, previewing));
    click(cx, "preview-close");
    assert!(!view.read_with(cx, previewing));
}

#[gpui::test]
async fn enter_on_a_folder_goes_into_it_and_closes_the_quick_look(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    click(cx, "entry-0");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "Projects").await;
    cx.simulate_keystrokes("enter");
    assert!(!view.read_with(cx, previewing));
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;
}

#[gpui::test]
async fn going_to_another_folder_closes_the_quick_look(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    click(cx, "entry-1");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "a.txt").await;
    cx.simulate_keystrokes("alt-up");
    view.condition::<WindowEvent>(cx, |view, _| view.folder() != Some(tree.path()))
        .await;
    assert!(!view.read_with(cx, previewing));
}

#[gpui::test]
async fn a_click_beside_what_is_shown_closes_the_quick_look(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    click(cx, "entry-1");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "a.txt").await;
    cx.run_until_parked();
    let scrim = cx.debug_bounds("preview").unwrap();
    cx.simulate_click(
        scrim.origin + gpui::point(px(4.0), px(4.0)),
        Modifiers::none(),
    );
    assert!(!view.read_with(cx, previewing));
}

#[gpui::test]
async fn a_fresh_read_keeps_the_quick_look_on_its_entry(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    click(cx, "entry-2");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "notes.txt").await;
    view.update(cx, |view, cx| {
        view.sort_by(nova_files_api::SortKey::Name);
        cx.notify();
    });
    view.condition::<WindowEvent>(cx, |view, _| {
        view.entry_names() == ["Projects", "notes.txt", "a.txt"]
    })
    .await;
    assert_eq!(previewed_name(&view, cx).as_deref(), Some("notes.txt"));
    cx.simulate_keystrokes("right");
    wait_for_preview(&view, cx, "a.txt").await;
}

#[gpui::test]
async fn any_cursor_move_takes_the_quick_look_along(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    click(cx, "entry-1");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "a.txt").await;
    cx.simulate_keystrokes("end");
    wait_for_preview(&view, cx, "notes.txt").await;
    cx.simulate_keystrokes("home");
    wait_for_preview(&view, cx, "Projects").await;
    assert_eq!(selected(&view, cx), ["Projects"]);
}

#[gpui::test]
async fn a_fresh_read_shows_the_file_as_it_is_now(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    click(cx, "entry-1");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "a.txt").await;
    fs::write(tree.path().join("a.txt"), "changed\n").unwrap();
    view.update(cx, |view, cx| {
        view.sort_by(nova_files_api::SortKey::Name);
        cx.notify();
    });
    view.condition::<WindowEvent>(cx, |view, cx| {
        previewed(view, cx)
            .and_then(|shown| shown.preview)
            .is_some_and(|preview| {
                preview.content
                    == nova_files_preview_api::PreviewContent::Text {
                        text: "changed\n".into(),
                        cut: false,
                    }
            })
    })
    .await;
}

#[gpui::test]
async fn stepping_starts_from_the_entry_shown(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview");
    let (view, cx) = with_text(cx, &tree).await;

    click(cx, "entry-0");
    click_with(cx, "entry-2", Modifiers::control());
    click_with(cx, "entry-2", Modifiers::control());
    assert_eq!(selected(&view, cx), ["Projects"]);
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "Projects").await;
    cx.simulate_keystrokes("right");
    wait_for_preview(&view, cx, "a.txt").await;
    assert_eq!(selected(&view, cx), ["a.txt"]);
}

#[gpui::test]
async fn a_space_inside_a_name_being_typed_does_not_open_the_quick_look(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview-jump");
    fs::write(tree.path().join("my music.txt"), "").unwrap();
    fs::write(tree.path().join("my notes.txt"), "").unwrap();
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx) = open_on(cx, tree.path()).await;

    cx.simulate_keystrokes("m y space n");
    assert_eq!(selected(&view, cx), ["my notes.txt"]);
    assert!(!view.read_with(cx, previewing));
}

#[gpui::test]
async fn each_kind_of_entry_is_drawn_its_own_way(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview-kinds");
    let (view, cx) = with_text(cx, &tree).await;
    let drawn = |cx: &mut VisualTestContext| {
        cx.run_until_parked();
        [
            "preview-picture",
            "preview-text",
            "preview-folder",
            "preview-icon",
        ]
        .into_iter()
        .filter(|selector| cx.debug_bounds(selector).is_some())
        .collect::<Vec<_>>()
    };

    click(cx, "entry-0");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "Projects").await;
    assert_eq!(drawn(cx), ["preview-folder"]);
    assert!(cx.debug_bounds("preview-more").is_none());

    cx.simulate_keystrokes("right");
    wait_for_preview(&view, cx, "a.txt").await;
    assert_eq!(drawn(cx), ["preview-text"]);
}

#[gpui::test]
async fn a_binary_file_shows_its_icon(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview-icon");
    fs::write(tree.path().join("a.bin"), [0, 1, 2, 0]).unwrap();
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx) = open_on(cx, tree.path()).await;

    click(cx, "entry-1");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "a.bin").await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("preview-icon").is_some());
    assert!(cx.debug_bounds("preview-text").is_none());
}

#[gpui::test]
async fn a_big_folder_says_how_many_more_it_holds(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview-more");
    for n in 0..12 {
        fs::write(tree.path().join("Projects").join(format!("{n}.txt")), "").unwrap();
    }
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx) = open_on(cx, tree.path()).await;

    click(cx, "entry-0");
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "Projects").await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("preview-folder").is_some());
    assert!(cx.debug_bounds("preview-more").is_some());
}

#[gpui::test]
async fn pictures_are_asked_to_fit_the_room_the_window_has(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview-fit");
    let (view, cx) = with_text(cx, &tree).await;
    let fit = |view: &Entity<MainWindow>, cx: &mut VisualTestContext| {
        cx.run_until_parked();
        view.read_with(cx, preview_fit)
    };

    let drawn = fit(&view, cx).unwrap();
    cx.simulate_resize(gpui::size(px(500.0), px(400.0)));
    let smaller = fit(&view, cx).unwrap();
    assert!(smaller.width < drawn.width && smaller.height < drawn.height);
    cx.simulate_resize(gpui::size(px(600.0), px(400.0)));
    let wider = fit(&view, cx).unwrap();
    assert!(wider.width > smaller.width);
    assert_eq!(wider.height, smaller.height);
}

async fn click_the_first_link<'a>(
    cx: &'a mut TestAppContext,
    tree: &TempFolder,
    note: &str,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    fs::write(tree.path().join("a.md"), note).unwrap();
    cx.update(nova_files_window::bind_window_keys);
    let (view, cx) = open_on(cx, tree.path()).await;
    cx.run_until_parked();
    let names = view.read_with(cx, |view, _| view.entry_names());
    let a = names.iter().position(|name| name == "a.md").unwrap();
    let selector: &'static str = format!("entry-{a}").leak();
    click(cx, selector);
    cx.simulate_keystrokes("space");
    wait_for_preview(&view, cx, "a.md").await;
    cx.run_until_parked();
    let page = cx.debug_bounds("preview-markdown").unwrap();
    counted_click(cx, page.origin + gpui::point(px(48.0), px(32.0)), 1);
    (view, cx)
}

#[gpui::test]
async fn a_link_to_a_note_beside_it_moves_the_quick_look_there(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview-link");
    fs::write(tree.path().join("b.md"), "# B\n").unwrap();
    let note = "[the b note](b.md)\n\nmore words\n";
    let (view, cx) = click_the_first_link(cx, &tree, note).await;
    wait_for_preview(&view, cx, "b.md").await;
    assert_eq!(selected(&view, cx), ["b.md"]);
}

#[gpui::test]
async fn a_link_to_a_folder_goes_into_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("preview-folder-link");
    let note = "[the projects](Projects/)\n";
    let (view, cx) = click_the_first_link(cx, &tree, note).await;
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;
    assert!(!view.read_with(cx, previewing));
}
