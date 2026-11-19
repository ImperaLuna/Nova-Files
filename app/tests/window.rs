#![expect(
    clippy::unwrap_used,
    clippy::future_not_send,
    reason = "test setup should fail loudly; GPUI test contexts live on one thread"
)]

mod common;
mod window_band;
mod window_drag;
mod window_gathered;
mod window_island;
mod window_jump;
mod window_keys;
mod window_opens_with;
mod window_path;
mod window_preview;
mod window_properties;
mod window_select;
mod window_sidebar;
mod window_start;
mod window_tabs;

use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gpui::{App, Entity, Modifiers, SharedString, TestAppContext, VisualTestContext, px};
use nova_files_api::{Color, EntryId, Fit};
use nova_files_picker_api::{PickAnswer, PickId, PickKind, PickReply, PickRequest};
use nova_files_places_ui::TreePart;
use nova_files_preview_ui::{PreviewPart, Previewed};
use nova_files_properties_ui::{PropertiesPage, PropertiesPart};
use nova_files_settings_api::{Settings, Thumbnails};
use nova_files_shell::Published;
use nova_files_trash_ui::TrashPart;
use nova_files_ui::{Colors, CoreStatus, Location, Look, MainWindow, WindowEvent};

use common::{TempFolder, Watchdog, click_with, double_click};

fn previewing(view: &MainWindow, cx: &App) -> bool {
    view.part::<PreviewPart>()
        .is_some_and(|part| part.read(cx).previewing())
}

fn previewed(view: &MainWindow, cx: &App) -> Option<Previewed> {
    view.part::<PreviewPart>()?.read(cx).previewed(cx)
}

fn preview_fit(view: &MainWindow, cx: &App) -> Option<Fit> {
    view.part::<PreviewPart>()?.read(cx).preview_fit()
}

async fn open_on<'a>(
    cx: &'a mut TestAppContext,
    folder: &Path,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    let (view, cx) = window_on(cx, folder, folder);
    wait_for_folder(&view, cx, folder).await;
    (view, cx)
}

fn window_on<'a>(
    cx: &'a mut TestAppContext,
    folder: &Path,
    home: &Path,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    window_showing(cx, folder, home, Vec::new())
}

fn window_showing<'a>(
    cx: &'a mut TestAppContext,
    folder: &Path,
    home: &Path,
    names: Vec<std::ffi::OsString>,
) -> (Entity<MainWindow>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    with_parts(cx);
    let core_home = nova_files_core::Home {
        folder: home.to_owned(),
        config: home.join(".config"),
        state: home.with_extension("state"),
        data: home.join(".local/share"),
        drive_trashes: false,
    };
    let core = nova_files_core::spawn_with(nova_files_core::IDLE_EXIT, Some(core_home), None, None)
        .unwrap();
    let (start, home) = (folder.to_owned(), home.to_owned());
    cx.add_window_view(|_, cx| {
        let mut view = MainWindow::with_home(&core.connector, &start, home, cx);
        view.select_on_arrival(start.clone(), names);
        view
    })
}

fn with_parts(cx: &TestAppContext) {
    cx.update(|cx| cx.set_global(nova_files::parts()));
}

fn trash_part(view: &MainWindow) -> Entity<TrashPart> {
    view.part::<TrashPart>().unwrap()
}

fn trash_count(view: &MainWindow, cx: &App) -> Option<usize> {
    trash_part(view).read(cx).count()
}

fn trash_open(view: &MainWindow, cx: &App) -> bool {
    let trash = trash_part(view);
    let folder = trash.read(cx).folder();
    folder.is_some() && view.folder() == folder
}

fn trash_was_in(view: &MainWindow, index: usize, cx: &App) -> Option<SharedString> {
    let listing = view.host().entries.read(cx).listing()?;
    trash_part(view)
        .read(cx)
        .was_in(listing.entries.get(index)?)
}

async fn open_trash(view: &Entity<MainWindow>, cx: &mut VisualTestContext, new_tab: bool) {
    let trash = view.read_with(cx, |view, _| trash_part(view));
    view.condition::<WindowEvent>(cx, |view, cx| trash_part(view).read(cx).folder().is_some())
        .await;
    trash.update(cx, |trash, cx| {
        if new_tab {
            trash.open_in_new_tab(cx);
        } else {
            trash.open(cx);
        }
    });
    cx.run_until_parked();
}

async fn wait_for_folder(view: &Entity<MainWindow>, cx: &VisualTestContext, folder: &Path) {
    view.condition::<WindowEvent>(cx, |view, _| view.folder() == Some(folder))
        .await;
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let bounds = cx.debug_bounds(selector).unwrap();
    cx.simulate_click(bounds.center(), Modifiers::none());
}

#[gpui::test]
async fn window_connects_to_the_core(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("window");
    let (view, cx) = open_on(cx, tree.path()).await;

    view.condition::<WindowEvent>(cx, |view, _| view.status() == CoreStatus::Connected)
        .await;
}

#[gpui::test]
fn window_without_a_core_still_opens(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let core = nova_files_core::spawn_with_idle_exit(Duration::ZERO).unwrap();
    core.thread.join().unwrap();

    let window = cx.add_window(|_, cx| MainWindow::new(&core.connector, Path::new("/"), cx));
    let view = window.root(cx).unwrap();

    view.read_with(cx, |view, _| assert_eq!(view.status(), CoreStatus::Gone));
}

#[gpui::test]
async fn shows_the_folder_it_opened_on(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("window");
    let (view, cx) = open_on(cx, tree.path()).await;

    view.read_with(cx, |view, _| {
        assert_eq!(view.entry_names(), ["Projects", "notes.txt"]);
    });
}

#[gpui::test]
async fn double_clicking_a_folder_goes_into_it_and_up_comes_back(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("window");
    let (view, cx) = open_on(cx, tree.path()).await;

    double_click(cx, "entry-0");
    let projects = tree.path().join("Projects");
    wait_for_folder(&view, cx, &projects).await;
    view.read_with(cx, |view, _| assert_eq!(view.entry_names(), ["nova"]));

    click(cx, "up");
    wait_for_folder(&view, cx, tree.path()).await;
}

#[gpui::test]
async fn clicking_a_file_selects_it_and_stays_put(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("window");
    let (view, cx) = open_on(cx, tree.path()).await;

    click(cx, "entry-1");
    cx.run_until_parked();

    view.read_with(cx, |view, _| {
        assert_eq!(view.folder(), Some(tree.path()));
        assert_eq!(view.selected_names(), ["notes.txt"]);
    });
}

#[gpui::test]
async fn unreadable_folder_leaves_the_window_where_it_was(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("window");
    let (view, cx) = open_on(cx, tree.path()).await;
    let gone = tree.path().join("Projects");
    fs::remove_dir_all(&gone).unwrap();

    let refused = Rc::new(RefCell::new(Vec::new()));
    let _watch = cx.update(|_, cx| {
        let refused = Rc::clone(&refused);
        cx.subscribe(&view, move |_, event: &WindowEvent, _| {
            if let WindowEvent::Refused(path) = event {
                refused.borrow_mut().push(path.clone());
            }
        })
    });

    double_click(cx, "entry-0");
    let seen = Rc::clone(&refused);
    view.condition::<WindowEvent>(cx, move |_, _| !seen.borrow().is_empty())
        .await;

    assert_eq!(*refused.borrow(), [gone]);
    view.read_with(cx, |view, _| {
        assert_eq!(view.folder(), Some(tree.path()));
        assert_eq!(view.entry_names(), ["Projects", "notes.txt"]);
    });
}

#[gpui::test]
async fn unreadable_start_folder_falls_back_to_home(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("window");
    let missing = tree.path().join("missing");
    let (view, cx) = window_on(cx, &missing, tree.path());

    wait_for_folder(&view, cx, tree.path()).await;
}

#[gpui::test]
async fn dots_in_a_path_are_tidied(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("window");
    let dotted = tree.path().join("Projects").join("..");
    let (view, cx) = window_on(cx, &dotted, tree.path());

    wait_for_folder(&view, cx, tree.path()).await;
}

#[gpui::test]
async fn clicking_a_place_opens_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("window");
    let home = tree.path().join("Projects");
    let (view, cx) = window_on(cx, tree.path(), &home);
    wait_for_folder(&view, cx, tree.path()).await;
    view.condition::<WindowEvent>(cx, |view, _| !view.places().is_empty())
        .await;

    click(cx, "place-0");

    wait_for_folder(&view, cx, &home).await;
}

#[gpui::test]
async fn columns_fill_in_after_the_names(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("window");
    let (view, cx) = open_on(cx, tree.path()).await;

    view.condition::<WindowEvent>(cx, |view, _| {
        view.columns(1).is_some_and(|text| !text.size.is_empty())
    })
    .await;

    view.read_with(cx, |view, _| {
        let folder = view.columns(0).unwrap();
        assert_ne!(folder.modified, "");
        assert_eq!(folder.size, "");
        let file = view.columns(1).unwrap();
        assert_eq!(file.size, "0 B");
        assert_eq!(view.columns(2), None);
    });
}

#[gpui::test]
async fn columns_line_up_under_their_headers(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("window");
    fs::write(tree.path().join("a much longer name.txt"), "").unwrap();
    let (_view, cx) = open_on(cx, tree.path()).await;
    cx.run_until_parked();

    let mut left = |selector| cx.debug_bounds(selector).unwrap().left();
    let header = left("modified-header");
    for row in ["modified-0", "modified-1", "modified-2"] {
        assert_eq!(left(row), header, "{row}");
    }
}

#[gpui::test]
async fn a_click_on_a_column_head_orders_the_list_by_it(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("window");
    fs::write(tree.path().join("big.txt"), "a good deal of text").unwrap();
    fs::write(tree.path().join("a.txt"), "a bit").unwrap();
    let (view, cx) = open_on(cx, tree.path()).await;
    let shows = async |cx: &mut VisualTestContext, names: [&str; 4]| {
        view.condition::<WindowEvent>(cx, move |view, _| view.entry_names() == names)
            .await;
    };

    cx.run_until_parked();
    assert!(cx.debug_bounds("arrow-name").is_some());
    assert!(cx.debug_bounds("arrow-size").is_none());
    let head = cx.debug_bounds("head-modified").unwrap().left();
    let column = cx.debug_bounds("modified-header").unwrap().left();
    assert!(head < column, "{head:?} {column:?}");
    let row = cx.debug_bounds("entry-0").unwrap();
    let first = cx.debug_bounds("head-name").unwrap().left() - row.left();
    let last = row.right() - cx.debug_bounds("head-size").unwrap().right();
    assert_eq!(first, last);

    click(cx, "head-size");
    shows(cx, ["Projects", "big.txt", "a.txt", "notes.txt"]).await;
    cx.run_until_parked();
    assert!(cx.debug_bounds("arrow-size").is_some());
    assert!(cx.debug_bounds("arrow-name").is_none());
    click(cx, "head-size");
    shows(cx, ["Projects", "notes.txt", "a.txt", "big.txt"]).await;
    click(cx, "head-name");
    shows(cx, ["Projects", "a.txt", "big.txt", "notes.txt"]).await;
    click(cx, "head-name");
    shows(cx, ["Projects", "notes.txt", "big.txt", "a.txt"]).await;
}

fn pick_request(kind: PickKind) -> (Box<PickRequest>, mpsc::Receiver<PickAnswer>) {
    let (sender, answers) = mpsc::channel();
    let request = PickRequest {
        id: PickId(9),
        kind,
        title: "Open".to_owned(),
        accept_label: None,
        filters: Vec::new(),
        filter: None,
        folder: None,
        parent: None,
        types: None,
        reply: PickReply::new(move |answer| {
            sender.send(answer).ok();
        }),
    };
    (Box::new(request), answers)
}

const OPEN_ONE: PickKind = PickKind::Open {
    multiple: false,
    folders: false,
};

#[gpui::test]
async fn a_picker_picks_the_file_double_clicked(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("window");
    let (view, cx) = open_on(cx, tree.path()).await;
    let (request, answers) = pick_request(OPEN_ONE);
    view.update_in(cx, |view, window, cx| view.set_pick(request, window, cx));

    click(cx, "entry-1");
    assert!(answers.try_recv().is_err());

    double_click(cx, "entry-1");
    assert_eq!(
        answers.try_recv(),
        Ok(PickAnswer::Picked {
            paths: vec![tree.path().join("notes.txt")],
            filter: None
        })
    );
}

#[gpui::test]
async fn a_picker_still_goes_into_folders(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("window");
    let (view, cx) = open_on(cx, tree.path()).await;
    let (request, answers) = pick_request(OPEN_ONE);
    view.update_in(cx, |view, window, cx| view.set_pick(request, window, cx));

    click(cx, "entry-0");
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;

    assert!(answers.try_recv().is_err());
    view.read_with(cx, |view, cx| assert_eq!(view.pick_id(cx), Some(PickId(9))));
}

#[gpui::test]
async fn a_picker_picks_nothing_from_a_listing_it_has_left(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("window");
    let (view, cx) = open_on(cx, tree.path()).await;
    let (request, answers) = pick_request(OPEN_ONE);
    view.update_in(cx, |view, window, cx| view.set_pick(request, window, cx));
    let first = view.read_with(cx, |view, _| view.listing_id()).unwrap();
    let notes = EntryId(1);

    view.update(cx, |view, _| view.open(&tree.path().join("Projects")));
    assert!(!view.update(cx, |view, cx| view.pick_entry(first, notes, cx)));
    wait_for_folder(&view, cx, &tree.path().join("Projects")).await;

    view.update(cx, |view, _| view.go_up());
    wait_for_folder(&view, cx, tree.path()).await;
    assert!(!view.update(cx, |view, cx| view.pick_entry(first, notes, cx)));

    assert!(answers.try_recv().is_err());
    let now = view.read_with(cx, |view, _| view.listing_id()).unwrap();
    assert!(view.update(cx, |view, cx| view.pick_entry(now, notes, cx)));
}

#[gpui::test]
async fn a_save_picker_leaves_double_clicked_files_alone(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("window");
    let (view, cx) = open_on(cx, tree.path()).await;
    let (request, answers) = pick_request(PickKind::Save {
        name: "a.txt".into(),
    });
    view.update_in(cx, |view, window, cx| view.set_pick(request, window, cx));

    double_click(cx, "entry-1");

    assert!(answers.try_recv().is_err());
    view.read_with(cx, |view, cx| assert_eq!(view.pick_id(cx), Some(PickId(9))));
}

#[gpui::test]
async fn closing_a_picker_answers_cancelled(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("window");
    let (view, cx) = open_on(cx, tree.path()).await;
    let (request, answers) = pick_request(OPEN_ONE);
    view.update_in(cx, |view, window, cx| view.set_pick(request, window, cx));

    let app: &mut TestAppContext = cx;
    app.update(|cx| nova_files_ui::close_picker(PickId(8), cx));
    cx.run_until_parked();
    assert!(answers.try_recv().is_err());

    let app: &mut TestAppContext = cx;
    app.update(|cx| nova_files_ui::close_picker(PickId(9), cx));
    assert_eq!(app.update(|cx| cx.windows().len()), 0);
    assert_eq!(answers.try_recv(), Ok(PickAnswer::Cancelled));
    drop(view);
}

#[gpui::test]
async fn a_hand_edit_to_the_theme_file_reaches_the_window(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("theme-edit");
    let (view, cx) = open_on(cx, tree.path()).await;
    let folder = tree.path().join(".config/nova-files");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("theme.toml"), "[dark]\nprimary = \"#123456\"\n").unwrap();
    let primary = gpui::rgb(Color(0x12_34_56).0);
    view.condition::<WindowEvent>(cx, move |_, cx| Colors::of(cx).accept == primary)
        .await;
}

#[gpui::test]
async fn a_hand_edit_to_the_settings_file_reaches_the_window(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("settings-edit");
    let (view, cx) = open_on(cx, tree.path()).await;
    let folder = tree.path().join(".config/nova-files");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("settings.toml"), "thumbnails = false\n").unwrap();
    view.condition::<WindowEvent>(cx, |_, cx| {
        Published::<Settings>::get(cx)
            .is_some_and(|settings| settings.thumbnails == Thumbnails::Hidden)
    })
    .await;
}

fn props<'a>(view: &MainWindow, cx: &'a App) -> &'a PropertiesPart {
    view.part::<PropertiesPart>().unwrap().read(cx)
}

fn properties_at(paths: Vec<std::path::PathBuf>) -> Location {
    Location::Page(nova_files_shell::PageRef::new(PropertiesPage::new(paths)))
}

fn tree_part(view: &MainWindow) -> Entity<TreePart> {
    view.part::<TreePart>().unwrap()
}
