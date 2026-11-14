//! The Start page as a place: a new window can open on it, Back and
//! Forward cross it, a folder that can't be read leaves the window on it,
//! and each folder a person opens reaches the core's recent folders.

use std::path::PathBuf;
use std::sync::mpsc;

use nova_files_api::{Command, Connector, Event, RecentId};

use super::*;

/// Opens a window on the Start page with a running core whose home is
/// `home`, and returns the core's connector too.
fn start_window<'a>(
    cx: &'a mut TestAppContext,
    home: &Path,
) -> (Entity<MainWindow>, &'a mut VisualTestContext, Connector) {
    window_at(cx, home, &Location::Start)
}

/// Opens a window at `location`, as [`start_window`] does.
fn window_at<'a>(
    cx: &'a mut TestAppContext,
    home: &Path,
    location: &Location,
) -> (Entity<MainWindow>, &'a mut VisualTestContext, Connector) {
    cx.executor().allow_parking();
    let core_home = nova_files_core::Home {
        folder: home.to_owned(),
        config: home.join(".config"),
        // Beside the tree, so a saved recent folder never shows in it.
        state: home.with_extension("state"),
        data: home.join(".local/share"),
        drive_trashes: false,
    };
    let core = nova_files_core::spawn_with(nova_files_core::IDLE_EXIT, Some(core_home), None, None)
        .unwrap();
    let connector = core.connector.clone();
    let home = home.to_owned();
    let (view, cx) =
        cx.add_window_view(|_, cx| MainWindow::at(&core.connector, location, home, cx));
    (view, cx, connector)
}

/// The recent folders the core keeps, newest first.
fn recent_folders(connector: &Connector) -> Vec<PathBuf> {
    let (sink, events) = mpsc::channel();
    let connection = connector.connect(sink).unwrap();
    connection
        .send(Command::RecentFolders {
            id: RecentId(0),
            count: 5,
            skip: Vec::new(),
        })
        .unwrap();
    match events.recv_timeout(common::WAIT).unwrap() {
        Event::RecentFolders { folders, .. } => {
            folders.into_iter().map(|folder| folder.path).collect()
        }
        other => vec![PathBuf::from(format!("not the recent folders: {other:?}"))],
    }
}

#[gpui::test]
async fn back_and_forward_cross_the_start_page(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("start-history");
    let projects = tree.path().join("Projects");
    let (view, cx, connector) = start_window(cx, tree.path());
    view.read_with(cx, |view, _| {
        assert!(view.is_on_start());
        assert_eq!(view.folder(), None);
    });

    view.update(cx, |view, _| view.open(&projects));
    wait_for_folder(&view, cx, &projects).await;
    view.read_with(cx, |view, _| {
        assert!(!view.is_on_start());
        assert_eq!(view.back_target(), Some(&Location::Start));
    });

    // Start needs nothing from the core: Back is there at once.
    view.update(cx, MainWindow::go_back);
    view.read_with(cx, |view, _| {
        assert!(view.is_on_start());
        assert_eq!(view.folder(), None);
        assert_eq!(view.entry_names(), Vec::<String>::new());
        assert_eq!(
            view.forward_target(),
            Some(&Location::from(projects.as_path()))
        );
    });

    view.update(cx, MainWindow::go_forward);
    wait_for_folder(&view, cx, &projects).await;
    assert_eq!(recent_folders(&connector), vec![projects]);
}

#[gpui::test]
async fn a_folder_that_cant_be_read_leaves_the_window_on_start(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("start-refused");
    let (view, cx, _connector) = start_window(cx, tree.path());
    view.update(cx, |view, _| view.open(&tree.path().join("missing")));
    view.condition::<WindowEvent>(cx, |view, _| !view.waiting())
        .await;
    view.read_with(cx, |view, _| {
        assert!(view.is_on_start());
        assert_eq!(view.back_target(), None);
    });
}

#[gpui::test]
fn going_to_start_drops_the_folder_on_its_way(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("start-drops");
    let projects = tree.path().join("Projects");
    let (view, cx, connector) = start_window(cx, tree.path());
    view.update(cx, |view, cx| {
        view.open(&projects);
        view.open_start(cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.is_on_start());
        assert!(!view.waiting());
        assert_eq!(view.back_target(), None);
    });
    assert_eq!(recent_folders(&connector), Vec::<PathBuf>::new());
}

#[gpui::test]
async fn a_recent_folder_card_opens_its_folder(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("start-cards");
    let projects = tree.path().join("Projects");
    let (view, cx, _connector) = start_window(cx, tree.path());
    view.update(cx, |view, _| view.open(&projects));
    wait_for_folder(&view, cx, &projects).await;
    view.update(cx, MainWindow::go_back);
    view.condition::<WindowEvent>(cx, |view, _| !view.recent_folders().is_empty())
        .await;
    view.read_with(cx, |view, _| {
        let newest = &view.recent_folders()[0];
        assert_eq!(newest.path, projects);
        assert_eq!(newest.entries, 1);
    });
    cx.run_until_parked();
    let card = cx.debug_bounds("recent-0").unwrap();
    cx.simulate_click(card.center(), Modifiers::none());
    wait_for_folder(&view, cx, &projects).await;
}

#[gpui::test]
async fn a_fallback_folder_isnt_counted_as_used(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("start-fallback");
    let missing = tree.path().join("missing");
    let (view, cx, connector) = window_at(cx, tree.path(), &Location::Folder(missing));
    wait_for_folder(&view, cx, tree.path()).await;
    assert_eq!(recent_folders(&connector), Vec::<PathBuf>::new());
}
