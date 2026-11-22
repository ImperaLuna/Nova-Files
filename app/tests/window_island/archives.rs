use std::os::unix::fs::PermissionsExt as _;

use super::menus::{choose, offered};
use super::*;
use crate::window_sidebar::right_click;

const MAKES_AN_EMPTY_ARCHIVE: &str = "#!/bin/sh\n: > \"$4\"\n";
pub(super) const HOLDS_ONE_FILE: &str = "#!/bin/sh\nprintf '%s\\000' f 0 5 0 u 420 inner.zip ''\n";

pub(super) fn fake_helper(home: &TempFolder, script: &str) -> nova_files::Archiver {
    let helper = home.path().join("archiver");
    fs::write(&helper, script).unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    nova_files::Archiver::at(helper)
}

pub(super) fn archive_tree(test: &str) -> TempFolder {
    let tree = TempFolder::tree(test);
    fs::write(tree.path().join("bundle.zip"), "").unwrap();
    tree
}

async fn wait_for_note_starting(view: &Entity<MainWindow>, cx: &VisualTestContext, start: &str) {
    view.condition::<WindowEvent>(cx, |view, _| {
        view.note()
            .is_some_and(|note| note.label.starts_with(start))
    })
    .await;
}

#[gpui::test]
async fn an_archive_offers_extract_in_its_own_group(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = archive_tree("archive-menu");
    let home = TempFolder::tree("archive-menu-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("extract-all").is_none());

    right_click(cx, "name-1");
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_names()),
        ["bundle.zip"]
    );
    assert_eq!(
        offered(&view, cx),
        (
            [
                "Open",
                "Preview",
                "Extract here",
                "Extract to…",
                "Cut",
                "Copy",
                "Rename",
                "Compress",
                "Compress as…",
                "Move to Trash",
                "Properties"
            ]
            .map(str::to_owned)
            .to_vec(),
            vec![2, 4, 9, 10]
        )
    );

    choose(&view, cx, "Extract here");
    wait_for_note_starting(&view, cx, "Couldn't extract bundle.zip").await;
}

#[gpui::test]
async fn extract_to_opens_a_folder_picker(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = archive_tree("archive-extract-to");
    let home = TempFolder::tree("archive-extract-to-home");
    let (view, cx) = island_window(cx, tree.path(), &home).await;
    let windows = cx.windows().len();

    right_click(cx, "name-1");
    choose(&view, cx, "Extract to…");
    cx.run_until_parked();

    assert_eq!(cx.windows().len(), windows + 1);
}

#[gpui::test]
async fn compress_names_the_archive_and_opens_its_rename_field(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = archive_tree("archive-compress");
    let home = TempFolder::tree("archive-compress-home");
    let archiver = fake_helper(&home, MAKES_AN_EMPTY_ARCHIVE);
    fs::write(tree.path().join("notes.zip"), "").unwrap();
    cx.update(|cx| {
        nova_files_window::bind_text_keys(cx);
        nova_files_window::bind_window_keys(cx);
    });
    let (view, cx) =
        window_with_archiver(cx, (tree.path(), home.path()), Vec::new(), Some(archiver));
    wait_for_folder(&view, cx, tree.path()).await;
    cx.run_until_parked();

    right_click(cx, "name-2");
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_names()),
        ["notes.txt"]
    );
    let labels = offered(&view, cx).0;
    assert!(!labels.iter().any(|label| label.starts_with("Extract")));
    choose(&view, cx, "Compress");
    wait_for_note(&view, cx, "Made notes (2).zip").await;
    view.condition::<WindowEvent>(cx, MainWindow::renaming)
        .await;
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_names()),
        ["notes (2).zip"]
    );
    assert!(tree.path().join("notes (2).zip").exists());
}

#[gpui::test]
async fn an_opened_archive_offers_extract_all_and_nothing_that_changes_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = archive_tree("archive-inside");
    let home = TempFolder::tree("archive-inside-home");
    let archiver = fake_helper(&home, HOLDS_ONE_FILE);
    let inside = tree.path().join("bundle.zip");
    let (view, cx) = window_with_archiver(cx, (&inside, home.path()), Vec::new(), Some(archiver));
    wait_for_folder(&view, cx, &inside).await;
    cx.run_until_parked();

    assert!(cx.debug_bounds("extract-all").is_some());
    right_click(cx, "name-0");
    let labels = offered(&view, cx).0;
    for changes in [
        "Compress",
        "Rename",
        "Cut",
        "Move to Trash",
        "Extract here",
        "Extract to…",
    ] {
        assert!(
            !labels.iter().any(|label| label == changes),
            "{changes} in {labels:?}"
        );
    }
    assert!(labels.iter().any(|label| label == "Copy"), "{labels:?}");
    cx.simulate_keystrokes("escape");
    click(cx, "extract-all");
    wait_for_note_starting(&view, cx, "Couldn't extract bundle.zip").await;
}

#[gpui::test]
async fn a_locked_archive_asks_for_its_password_on_the_island(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = archive_tree("archive-locked");
    let home = TempFolder::tree("archive-locked-home");
    let given = home.path().join("given");
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\nlist) printf '%s\\000' f 1 5 0 u 420 inner.txt '' ;;\n*) read -r given; printf '%s' \"$given\" > '{}' ;;\nesac\n",
        given.display()
    );
    let archiver = fake_helper(&home, &script);
    cx.update(|cx| {
        nova_files_window::bind_text_keys(cx);
        nova_files_window::bind_window_keys(cx);
    });
    let (view, cx) =
        window_with_archiver(cx, (tree.path(), home.path()), Vec::new(), Some(archiver));
    wait_for_folder(&view, cx, tree.path()).await;
    cx.run_until_parked();

    right_click(cx, "name-1");
    choose(&view, cx, "Extract here");
    view.condition::<WindowEvent>(cx, |view, _| view.newest_question().is_some())
        .await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("island-password").is_some());
    assert!(cx.debug_bounds("island-unlock").is_some());

    cx.simulate_input("secret");
    cx.simulate_keystrokes("enter");
    view.condition::<WindowEvent>(cx, |view, _| view.note().is_some())
        .await;
    assert_eq!(fs::read_to_string(&given).unwrap(), "secret");
    assert!(cx.debug_bounds("island-password").is_none());
}

#[gpui::test]
async fn compress_as_picks_the_format_the_next_compress_uses(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = archive_tree("archive-compress-as");
    let home = TempFolder::tree("archive-compress-as-home");
    let archiver = fake_helper(&home, MAKES_AN_EMPTY_ARCHIVE);
    cx.update(|cx| {
        nova_files_window::bind_text_keys(cx);
        nova_files_window::bind_window_keys(cx);
    });
    let (view, cx) =
        window_with_archiver(cx, (tree.path(), home.path()), Vec::new(), Some(archiver));
    wait_for_folder(&view, cx, tree.path()).await;
    cx.run_until_parked();

    right_click(cx, "name-2");
    choose(&view, cx, "Compress as…");
    cx.run_until_parked();
    assert!(cx.debug_bounds("compress-7z").is_none());
    assert!(cx.debug_bounds("compress-more-rows").is_none());
    click(cx, "compress-more");
    let island = cx.debug_bounds("island").unwrap();
    let make = cx.debug_bounds("compress-make").unwrap();
    assert!(
        make.right() <= island.right(),
        "{make:?} outside {island:?}"
    );
    assert!(cx.debug_bounds("compress-deflate").is_some());
    assert!(cx.debug_bounds("compress-lock").is_some());
    click(cx, "compress-tar-xz");
    click(cx, "compress-cancel");
    assert!(cx.debug_bounds("compress-make").is_none());
    right_click(cx, "name-2");
    choose(&view, cx, "Compress as…");
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(cx.debug_bounds("compress-make").is_none());
    right_click(cx, "name-2");
    choose(&view, cx, "Compress as…");
    cx.run_until_parked();
    click(cx, "compress-more");
    click(cx, "compress-tar-gz");
    assert!(cx.debug_bounds("compress-deflate").is_none());
    assert!(cx.debug_bounds("compress-lock").is_none());
    click(cx, "compress-make");
    wait_for_note(&view, cx, "Made notes.tar.gz").await;
    assert!(cx.debug_bounds("compress-make").is_none());
    assert!(tree.path().join("notes.tar.gz").exists());
    view.condition::<WindowEvent>(cx, MainWindow::renaming)
        .await;
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    right_click(cx, "name-1");
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_names()),
        ["bundle.zip"]
    );
    choose(&view, cx, "Compress");
    wait_for_note(&view, cx, "Made bundle.tar.gz").await;
}

#[gpui::test]
async fn extract_all_leaves_with_the_archive(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = archive_tree("archive-leave");
    let home = TempFolder::tree("archive-leave-home");
    let archiver = fake_helper(&home, HOLDS_ONE_FILE);
    let inside = tree.path().join("bundle.zip");
    let (view, cx) = window_with_archiver(cx, (&inside, home.path()), Vec::new(), Some(archiver));
    wait_for_folder(&view, cx, &inside).await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("extract-all").is_some());

    view.update(cx, MainWindow::open_start);
    cx.run_until_parked();

    assert!(cx.debug_bounds("extract-all").is_none());
}
