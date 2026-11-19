use nova_files_api::Operation;
use nova_files_search_api::{Command as SearchCommand, Event as SearchEvent, SearchFailed};

use super::*;

fn next_find(core: &mut Core) -> Option<(ListingId, String, Option<PathBuf>)> {
    while let Ok(message) = core.messages.try_recv() {
        if let CoreMessage::Connected { events, .. } = message {
            core.events = Some(events);
        } else if let Some(SearchCommand::Find { id, query, within }) =
            message.into_command::<SearchCommand>()
        {
            return Some((id, query, within));
        }
    }
    None
}

fn answer_found(core: &Core, id: ListingId, found: &[(&str, &str, EntryKind)]) {
    let entries: Arc<[Entry]> = found
        .iter()
        .zip(0..)
        .map(|(&(_, name, kind), index)| Entry {
            id: EntryId(index),
            name: name.into(),
            kind,
        })
        .collect();
    let folders: Arc<[PathBuf]> = found
        .iter()
        .map(|&(folder, ..)| PathBuf::from(folder))
        .collect();
    core.send(Event::FolderListed(Listing {
        id,
        path: PathBuf::from(SHOWN),
        entries,
        folders: Some(folders),
    }));
}

const REPORTS: &[(&str, &str, EntryKind)] = &[
    ("/docs", "report.pdf", EntryKind::File),
    ("/old", "reports", EntryKind::Folder),
];

fn searching(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    cx.update(|cx| {
        nova_files_ui::bind_window_keys(cx);
        nova_files_ui::bind_text_keys(cx);
    });
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    cx.simulate_keystrokes("/");
    cx.run_until_parked();
    (view, cx, core)
}

fn found_reports(cx: &mut VisualTestContext, core: &mut Core, query: &str) -> ListingId {
    cx.simulate_input(query);
    cx.run_until_parked();
    let mut last = None;
    while let Some((id, ..)) = next_find(core) {
        last = Some(id);
    }
    let id = last.unwrap();
    answer_found(core, id, REPORTS);
    cx.run_until_parked();
    id
}

fn names(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, _| view.entry_names())
}

#[gpui::test]
fn typing_finds_everywhere_and_the_window_stays_in_its_folder(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.list_drop_folder().map(Path::to_path_buf)),
        Some(PathBuf::from(SHOWN))
    );

    cx.simulate_input("rep");
    cx.run_until_parked();
    let (id, query, within) = next_find(&mut core).unwrap();
    assert_eq!((query.as_str(), within), ("r", None));
    assert!(cx.debug_bounds("match-count").is_none());
    answer_found(&core, id, REPORTS);
    let (id, ..) = std::iter::from_fn(|| next_find(&mut core)).last().unwrap();
    answer_found(&core, id, REPORTS);
    cx.run_until_parked();

    assert_eq!(names(&view, cx), ["report.pdf", "reports"]);
    view.read_with(cx, |view, cx| {
        assert!(view.searching());
        assert_eq!(view.query(cx), "rep");
        assert_eq!(view.folder(), Some(Path::new(SHOWN)));
        assert_eq!(view.list_drop_folder(), None);
    });
    assert!(cx.debug_bounds("match-count").is_some());
    assert!(cx.debug_bounds("no-matches").is_none());
    assert!(core.next_listing().is_none());
}

#[gpui::test]
fn an_older_find_answering_late_is_dropped(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    cx.simulate_input("a");
    cx.run_until_parked();
    let (older, ..) = next_find(&mut core).unwrap();
    cx.simulate_input("b");
    cx.run_until_parked();
    let (newer, query, _) = next_find(&mut core).unwrap();
    assert_eq!(query, "ab");

    answer_found(&core, older, &REPORTS[..1]);
    cx.run_until_parked();
    assert_eq!(names(&view, cx), ["0", "1", "2"]);
    answer_found(&core, newer, REPORTS);
    cx.run_until_parked();

    assert_eq!(names(&view, cx), ["report.pdf", "reports"]);
    assert!(core.next_listing().is_none());
    assert!(next_find(&mut core).is_none());
}

#[gpui::test]
fn escape_clears_the_box_and_reads_the_folder_again(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    found_reports(cx, &mut core, "rep");

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    let (_, path) = core.next_listing().unwrap();
    assert_eq!(path, Path::new(SHOWN));
    view.read_with(cx, |view, cx| {
        assert!(!view.searching());
        assert_eq!(view.query(cx), "");
    });
}

#[gpui::test]
fn emptying_the_box_ends_the_search(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    found_reports(cx, &mut core, "r");

    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();

    let (_, path) = core.next_listing().unwrap();
    assert_eq!(path, Path::new(SHOWN));
    assert!(next_find(&mut core).is_none());
    view.read_with(cx, |view, _| assert!(!view.searching()));
}

#[gpui::test]
fn a_found_folder_opens_where_it_is_and_ends_the_search(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    found_reports(cx, &mut core, "rep");

    cx.simulate_keystrokes("down down enter");
    cx.run_until_parked();
    let (id, path) = core.next_listing().unwrap();
    assert_eq!(path, Path::new("/old/reports"));
    core.send(Event::FolderListed(Listing {
        id,
        path,
        entries: Arc::new([]),
        folders: None,
    }));
    cx.run_until_parked();
    view.read_with(cx, |view, cx| {
        assert!(!view.searching());
        assert_eq!(view.query(cx), "");
    });
}

#[gpui::test]
fn down_moves_into_the_results_and_enter_opens_a_file_where_it_is(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    found_reports(cx, &mut core, "rep");

    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert_eq!(view.cursor(), Some(0)));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    let opened: Vec<PathBuf> = core
        .sent::<nova_files_open_api::Command>()
        .into_iter()
        .filter_map(|command| match command {
            nova_files_open_api::Command::OpenFile { path, .. } => Some(path),
            nova_files_open_api::Command::OpenWith(_) => None,
        })
        .collect();
    assert_eq!(opened, [PathBuf::from("/docs/report.pdf")]);
}

#[gpui::test]
fn deleting_a_result_for_good_names_it_where_it_is(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    found_reports(cx, &mut core, "rep");

    cx.simulate_keystrokes("down shift-delete");
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.confirm().is_some()));
    let yes = cx.debug_bounds("island-yes").unwrap().center();
    cx.simulate_click(yes, gpui::Modifiers::none());
    cx.run_until_parked();

    let deleted: Vec<_> = core
        .commands()
        .into_iter()
        .filter_map(|command| match command {
            Command::StartJob {
                operation: Operation::Delete { paths },
                ..
            } => Some(paths),
            _ => None,
        })
        .collect();
    assert_eq!(deleted, [vec![PathBuf::from("/docs/report.pdf")]]);
}

#[gpui::test]
fn ctrl_f_puts_the_caret_back_in_the_box(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    found_reports(cx, &mut core, "rep");
    cx.simulate_keystrokes("down");
    cx.run_until_parked();

    cx.simulate_keystrokes("ctrl-f");
    cx.run_until_parked();
    cx.simulate_input("x");
    cx.run_until_parked();
    view.read_with(cx, |view, cx| assert_eq!(view.query(cx), "x"));
}

#[gpui::test]
fn nothing_found_shows_no_matches(cx: &mut TestAppContext) {
    let (_view, cx, mut core) = searching(cx);
    cx.simulate_input("zzz");
    cx.run_until_parked();
    let (id, ..) = std::iter::from_fn(|| next_find(&mut core)).last().unwrap();
    core.send(SearchEvent::FindFailed {
        id,
        reason: SearchFailed::IndexFailed,
    });
    cx.run_until_parked();

    assert!(cx.debug_bounds("no-matches").is_some());
    assert!(cx.debug_bounds("match-count").is_none());
}

#[gpui::test]
fn column_heads_do_nothing_while_searching(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    found_reports(cx, &mut core, "rep");

    view.update(cx, |view, _| view.sort_by(SortKey::Size));
    assert!(core.next_listing().is_none());
    view.read_with(cx, |view, _| assert!(view.searching()));
}

#[gpui::test]
fn a_folder_that_cant_be_read_leaves_the_search_as_it_was(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    found_reports(cx, &mut core, "rep");

    view.update(cx, |view, _| view.open(Path::new(MISSING)));
    let (id, path) = core.next_listing().unwrap();
    core.refuse(id, &path);
    cx.run_until_parked();

    assert_eq!(names(&view, cx), ["report.pdf", "reports"]);
    view.read_with(cx, |view, cx| {
        assert!(view.searching());
        assert_eq!(view.query(cx), "rep");
        assert_eq!(view.folder(), Some(Path::new(SHOWN)));
    });
}

#[gpui::test]
fn a_search_typed_while_a_folder_is_on_its_way_starts_there(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    view.update(cx, |view, _| view.open(Path::new(OTHER)));
    core.next_listing().unwrap();

    found_reports(cx, &mut core, "rep");
    view.read_with(cx, |view, _| {
        assert!(!view.waiting());
        assert_eq!(view.folder(), Some(Path::new(OTHER)));
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    let (_, path) = core.next_listing().unwrap();
    assert_eq!(path, Path::new(OTHER));
}

#[gpui::test]
fn found_files_get_a_folder_column_and_a_folder_only_does_not(cx: &mut TestAppContext) {
    let (_view, cx, mut core) = searching(cx);
    cx.run_until_parked();
    assert!(cx.debug_bounds("folder-header").is_none());
    assert!(cx.debug_bounds("scope").is_none());

    found_reports(cx, &mut core, "rep");

    assert!(cx.debug_bounds("folder-header").is_some());
    assert!(cx.debug_bounds("folder-1").is_some());
    assert!(cx.debug_bounds("scope").is_some());
    let head = cx.debug_bounds("modified-header").unwrap();
    let cell = cx.debug_bounds("modified-0").unwrap();
    assert_eq!(head.origin.x, cell.origin.x);
    let head = cx.debug_bounds("folder-header").unwrap();
    let link = cx.debug_bounds("folder-0").unwrap();
    assert!(link.origin.x < head.origin.x);
}

#[gpui::test]
fn a_folder_cell_goes_there_with_the_file_selected(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    found_reports(cx, &mut core, "rep");

    let link = cx.debug_bounds("folder-0").unwrap();
    cx.simulate_click(link.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let (id, path) = core.next_listing().unwrap();
    assert_eq!(path, Path::new("/docs"));
    let entries: Arc<[Entry]> = ["notes.txt", "report.pdf"]
        .into_iter()
        .zip(0..)
        .map(|(name, index)| Entry {
            id: EntryId(index),
            name: name.into(),
            kind: EntryKind::File,
        })
        .collect();
    core.send(Event::FolderListed(Listing {
        id,
        path,
        entries,
        folders: None,
    }));
    cx.run_until_parked();

    view.read_with(cx, |view, cx| {
        assert_eq!(view.folder(), Some(Path::new("/docs")));
        assert_eq!(view.selected_names(), ["report.pdf"]);
        assert!(!view.searching());
        assert_eq!(view.query(cx), "");
    });
}

#[gpui::test]
fn the_chip_switches_to_the_folder_searched_from_and_back(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    found_reports(cx, &mut core, "rep");

    let chip = cx.debug_bounds("scope").unwrap();
    cx.simulate_click(chip.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let (_, query, within) = next_find(&mut core).unwrap();
    assert_eq!(
        (query.as_str(), within),
        ("rep", Some(PathBuf::from(SHOWN)))
    );
    view.read_with(cx, |view, _| {
        assert_eq!(view.scope(), nova_files_ui::Scope::HereOnly);
    });

    let chip = cx.debug_bounds("scope").unwrap();
    cx.simulate_click(chip.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let (.., within) = next_find(&mut core).unwrap();
    assert_eq!(within, None);
}

#[gpui::test]
fn escape_before_any_find_answers_reads_the_folder_again(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    cx.simulate_input("r");
    cx.run_until_parked();
    next_find(&mut core).unwrap();

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    let (_, path) = core.next_listing().unwrap();
    assert_eq!(path, Path::new(SHOWN));
    view.read_with(cx, |view, _| assert!(!view.searching()));
}

#[gpui::test]
fn clicking_a_result_takes_the_keys_from_the_box(cx: &mut TestAppContext) {
    let (_view, cx, mut core) = searching(cx);
    found_reports(cx, &mut core, "rep");

    let row = cx.debug_bounds("entry-0").unwrap();
    cx.simulate_click(row.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    let opened = core
        .sent::<nova_files_open_api::Command>()
        .into_iter()
        .any(|command| matches!(command, nova_files_open_api::Command::OpenFile { path, .. } if path == Path::new("/docs/report.pdf")));
    assert!(opened);
}

#[gpui::test]
fn a_find_overtaken_by_a_folder_is_dropped_and_a_refusal_finds_again(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    found_reports(cx, &mut core, "rep");
    cx.simulate_input("o");
    cx.run_until_parked();
    let (overtaken, ..) = next_find(&mut core).unwrap();

    view.update(cx, |view, _| view.open(Path::new(MISSING)));
    let (id, path) = core.next_listing().unwrap();
    answer_found(&core, overtaken, &REPORTS[..1]);
    cx.run_until_parked();
    assert_eq!(names(&view, cx), ["report.pdf", "reports"]);

    core.refuse(id, &path);
    cx.run_until_parked();
    let (_, query, _) = next_find(&mut core).unwrap();
    assert_eq!(query, "repo");
    assert!(core.next_listing().is_none());
}

#[gpui::test]
fn slash_with_a_modifier_leaves_the_box_alone(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    found_reports(cx, &mut core, "rep");
    cx.simulate_keystrokes("down");
    cx.run_until_parked();

    let function = gpui::Modifiers {
        function: true,
        ..gpui::Modifiers::default()
    };
    for modifiers in [gpui::Modifiers::alt(), function] {
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: gpui::Keystroke {
                modifiers,
                key: "/".into(),
                key_char: Some("/".into()),
            },
            is_held: false,
            prefer_character_input: false,
        });
        cx.run_until_parked();
    }
    cx.simulate_input("x");
    cx.run_until_parked();
    view.read_with(cx, |view, cx| assert_eq!(view.query(cx), "rep"));
}

#[gpui::test]
fn an_older_find_that_fails_late_is_dropped(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    cx.simulate_input("a");
    cx.run_until_parked();
    let (older, ..) = next_find(&mut core).unwrap();
    cx.simulate_input("b");
    cx.run_until_parked();
    next_find(&mut core).unwrap();

    core.send(SearchEvent::FindFailed {
        id: older,
        reason: SearchFailed::IndexFailed,
    });
    cx.run_until_parked();

    assert_eq!(names(&view, cx), ["0", "1", "2"]);
    assert!(cx.debug_bounds("no-matches").is_none());
}

#[gpui::test]
fn a_refusal_before_any_results_reads_the_folder_again(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    cx.simulate_input("r");
    cx.run_until_parked();
    let (overtaken, ..) = next_find(&mut core).unwrap();
    view.update(cx, |view, _| view.open(Path::new(MISSING)));
    let (id, path) = core.next_listing().unwrap();
    answer_found(&core, overtaken, REPORTS);
    cx.run_until_parked();

    core.refuse(id, &path);
    cx.run_until_parked();
    let (_, again) = core.next_listing().unwrap();
    assert_eq!(again, Path::new(SHOWN));
}

#[gpui::test]
fn a_space_typed_into_the_box_stays_in_it(cx: &mut TestAppContext) {
    let (view, cx, _core) = searching(cx);
    cx.simulate_input("a");
    cx.simulate_keystrokes("space");
    cx.simulate_input("b");
    cx.run_until_parked();
    view.read_with(cx, |view, cx| {
        assert_eq!(view.query(cx), "a b");
        assert!(!view.previewing());
    });
}

#[gpui::test]
fn hidden_files_shown_mid_search_leave_the_search_alone(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    found_reports(cx, &mut core, "rep");
    for hidden in [HiddenFiles::Left, HiddenFiles::Shown] {
        let settings = Settings::default().with(SettingChange::HiddenFiles(hidden));
        core.send(settings_api::Event::Settings(Arc::new(settings)));
        cx.run_until_parked();
    }
    assert!(core.next_listing().is_none());
    assert_eq!(names(&view, cx), ["report.pdf", "reports"]);
}

#[gpui::test]
fn search_results_show_as_details_whatever_their_folder_shows(cx: &mut TestAppContext) {
    use nova_files_folders_api::{
        Command as ViewsCommand, Event as ViewsEvent, FolderView, IconSize, ViewMode,
    };

    let (view, cx, mut core) = searching(cx);
    let icons = FolderView {
        mode: ViewMode::Icons,
        icons: IconSize::Medium,
    };
    core.send(ViewsEvent::Views(Arc::from([(
        PathBuf::from(SHOWN),
        icons,
    )])));
    found_reports(cx, &mut core, "rep");
    assert_eq!(
        view.read_with(cx, |view, _| view.folder_view()),
        FolderView::default()
    );

    core.sent::<ViewsCommand>();
    cx.simulate_keystrokes("down ctrl-2");
    let kept = core
        .sent::<ViewsCommand>()
        .into_iter()
        .any(|command| matches!(command, ViewsCommand::SetView { .. }));
    assert!(!kept);
}
