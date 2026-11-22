use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::archives::{HOLDS_ONE_FILE, archive_tree, fake_helper};
use nova_files_shell::CommandSpec;

use super::*;

const SETTLE_ROUNDS: usize = 3;

#[derive(Debug, Clone, Copy)]
enum Picked {
    Nothing,
    All,
    One(usize),
}

fn files_under(roots: &[PathBuf]) -> BTreeMap<PathBuf, Option<u64>> {
    let mut found = BTreeMap::new();
    let mut left = roots.to_vec();
    while let Some(folder) = left.pop() {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = path.symlink_metadata() else {
                continue;
            };
            if meta.is_dir() {
                found.insert(path.clone(), None);
                left.push(path);
            } else {
                found.insert(path, Some(meta.len()));
            }
        }
    }
    found
}

fn picks(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Vec<Picked> {
    let count = view.read_with(cx, |view, _| view.entry_names().len());
    [Picked::Nothing, Picked::All]
        .into_iter()
        .chain((0..count).map(Picked::One))
        .collect()
}

async fn stand(view: &Entity<MainWindow>, cx: &mut VisualTestContext, at: &Path, picked: Picked) {
    cx.simulate_keystrokes("escape escape");
    view.update(cx, |view, _| view.open(at));
    wait_for_folder(view, cx, at).await;
    view.condition::<WindowEvent>(cx, |view, _| !view.entry_names().is_empty())
        .await;
    cx.simulate_keystrokes("ctrl-1");
    cx.run_until_parked();
    match picked {
        Picked::Nothing => {
            click(cx, "name-0");
            cx.simulate_keystrokes("escape");
        }
        Picked::All => {
            click(cx, "name-0");
            cx.simulate_keystrokes("ctrl-a");
        }
        Picked::One(row) => click(cx, Box::leak(format!("name-{row}").into_boxed_str())),
    }
    cx.run_until_parked();
}

fn offered(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Vec<&'static CommandSpec> {
    view.read_with(cx, MainWindow::offered)
}

async fn offered_in(
    view: &Entity<MainWindow>,
    cx: &mut VisualTestContext,
    at: &Path,
) -> BTreeSet<&'static str> {
    let mut ids = BTreeSet::new();
    stand(view, cx, at, Picked::Nothing).await;
    for picked in picks(view, cx) {
        stand(view, cx, at, picked).await;
        ids.extend(offered(view, cx).into_iter().map(|spec| spec.id));
    }
    ids
}

async fn settle(view: &Entity<MainWindow>, cx: &mut VisualTestContext) {
    for _ in 0..SETTLE_ROUNDS {
        cx.run_until_parked();
        if view.read_with(cx, |view, _| view.confirm().is_some()) {
            click(cx, "island-yes");
        }
        view.condition::<WindowEvent>(cx, |view, _| view.jobs_pending() == 0)
            .await;
    }
    cx.run_until_parked();
}

async fn changes_no_files(
    view: &Entity<MainWindow>,
    cx: &mut VisualTestContext,
    (at, kept): (&Path, &[PathBuf]),
    everywhere: &BTreeSet<&'static str>,
) -> usize {
    let mut ran = BTreeSet::new();
    stand(view, cx, at, Picked::Nothing).await;
    for picked in picks(view, cx) {
        stand(view, cx, at, picked).await;
        let here = offered(view, cx);
        for spec in here.into_iter().filter(|spec| everywhere.contains(spec.id)) {
            if !ran.insert(spec.id) {
                continue;
            }
            let before = files_under(kept);
            stand(view, cx, at, picked).await;
            cx.update(|window, cx| window.dispatch_action(spec.action(), cx));
            cx.run_until_parked();
            cx.simulate_keystrokes("x enter");
            settle(view, cx).await;
            assert_eq!(
                files_under(kept),
                before,
                "{} with {picked:?} in {} changed files",
                spec.label,
                at.display()
            );
        }
    }
    ran.len()
}

async fn trashed_window<'a>(
    cx: &'a mut TestAppContext,
    tree: &TempFolder,
    home: &TempFolder,
) -> (Entity<MainWindow>, &'a mut VisualTestContext, PathBuf) {
    let (view, cx) = island_window(cx, tree.path(), home).await;
    click(cx, "name-1");
    cx.simulate_keystrokes("delete");
    wait_for_note(&view, cx, "Moved notes.txt to Trash").await;
    click(cx, "name-0");
    cx.simulate_keystrokes("delete");
    wait_for_note(&view, cx, "Moved Projects to Trash").await;
    open_trash(&view, cx, false).await;
    wait_for_names(&view, cx, &["Projects", "notes.txt"]).await;
    let trash = view.read_with(cx, |view, cx| {
        trash_part(view).read(cx).folder().unwrap().to_owned()
    });
    (view, cx, trash)
}

async fn everywhere_else(
    view: &Entity<MainWindow>,
    cx: &mut VisualTestContext,
    tree: &Path,
) -> BTreeSet<&'static str> {
    fs::create_dir_all(tree.join("Work")).unwrap();
    fs::write(tree.join("plan.txt"), "plan").unwrap();
    stand(view, cx, tree, Picked::Nothing).await;
    let plan = view.read_with(cx, |view, _| {
        view.entry_names()
            .iter()
            .position(|name| name == "plan.txt")
    });
    stand(view, cx, tree, Picked::One(plan.unwrap())).await;
    cx.simulate_keystrokes("ctrl-c");
    offered_in(view, cx, tree).await
}

#[gpui::test]
async fn nothing_offered_in_the_trash_changes_files_unless_the_trash_offers_it(
    cx: &mut TestAppContext,
) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("allows-trash");
    let home = TempFolder::tree("allows-trash-home");
    let (view, cx, trash) = trashed_window(cx, &tree, &home).await;
    let everywhere = everywhere_else(&view, cx, tree.path()).await;
    let kept = [tree.path().to_owned(), trash.parent().unwrap().to_owned()];
    let ran = changes_no_files(&view, cx, (&trash, &kept), &everywhere).await;
    assert!(ran > 0, "nothing was offered to run");
}

#[gpui::test]
async fn nothing_offered_in_a_trashed_folder_changes_files(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("allows-trashed");
    let home = TempFolder::tree("allows-trashed-home");
    let (view, cx, trash) = trashed_window(cx, &tree, &home).await;
    let everywhere = everywhere_else(&view, cx, tree.path()).await;
    let kept = [tree.path().to_owned(), trash.parent().unwrap().to_owned()];
    let ran = changes_no_files(&view, cx, (&trash.join("Projects"), &kept), &everywhere).await;
    assert!(ran > 0, "nothing was offered to run");
}

#[gpui::test]
async fn nothing_offered_inside_an_archive_changes_files_unless_archives_offer_it(
    cx: &mut TestAppContext,
) {
    let _watchdog = Watchdog::start();
    let tree = archive_tree("allows-archive");
    let home = TempFolder::tree("allows-archive-home");
    let archiver = fake_helper(&home, HOLDS_ONE_FILE);
    let inside = tree.path().join("bundle.zip");
    cx.update(|cx| {
        nova_files_window::bind_text_keys(cx);
        nova_files_window::bind_window_keys(cx);
    });
    let (view, cx) = window_with_archiver(cx, (&inside, home.path()), Vec::new(), Some(archiver));
    wait_for_folder(&view, cx, &inside).await;
    let everywhere = everywhere_else(&view, cx, tree.path()).await;
    let kept = [tree.path().to_owned()];
    let ran = changes_no_files(&view, cx, (&inside, &kept), &everywhere).await;
    assert!(ran > 0, "nothing was offered to run");
}
