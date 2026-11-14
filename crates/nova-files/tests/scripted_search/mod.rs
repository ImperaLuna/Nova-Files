//! Searching against a scripted core: what is typed in the box is asked
//! for as a find, the files found take the list's place while the window
//! stays in its folder, and clearing the box or going elsewhere ends it.

use super::*;

/// The next find the window asks for, skipping other commands.
fn next_find(core: &mut Core) -> Option<(ListingId, String, Option<PathBuf>)> {
    while let Ok(message) = core.messages.try_recv() {
        match message {
            CoreMessage::Connected { events, .. } => core.events = Some(events),
            CoreMessage::Command {
                command: Command::Find { id, query, within },
                ..
            } => return Some((id, query, within)),
            _ => {}
        }
    }
    None
}

/// Answers find `id` with `found`: `(folder, name, kind)` each.
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

/// A window on [`SHOWN`] with its keys bound, its folder on screen and
/// the caret in the search box.
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

/// Types `query` and answers its find with [`REPORTS`].
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

    cx.simulate_input("rep");
    cx.run_until_parked();
    let (id, query, within) = next_find(&mut core).unwrap();
    assert_eq!((query.as_str(), within), ("r", None));
    answer_found(&core, id, REPORTS);
    let (id, ..) = std::iter::from_fn(|| next_find(&mut core)).last().unwrap();
    answer_found(&core, id, REPORTS);
    cx.run_until_parked();

    assert_eq!(names(&view, cx), ["report.pdf", "reports"]);
    view.read_with(cx, |view, cx| {
        assert!(view.searching());
        assert_eq!(view.query(cx), "rep");
        assert_eq!(view.folder(), Some(Path::new(SHOWN)));
    });
    assert!(cx.debug_bounds("match-count").is_some());
    assert!(cx.debug_bounds("no-matches").is_none());
    // Nothing else asked: the folder isn't read again.
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
        .commands()
        .into_iter()
        .filter_map(|command| match command {
            Command::OpenFile { path, .. } => Some(path),
            _ => None,
        })
        .collect();
    assert_eq!(opened, [PathBuf::from("/docs/report.pdf")]);
}

#[gpui::test]
fn ctrl_f_puts_the_caret_back_in_the_box(cx: &mut TestAppContext) {
    let (view, cx, mut core) = searching(cx);
    found_reports(cx, &mut core, "rep");
    cx.simulate_keystrokes("down");
    cx.run_until_parked();

    cx.simulate_keystrokes("ctrl-f");
    cx.run_until_parked();
    // The whole text is selected: typing replaces it.
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
    core.send(Event::FindFailed {
        id,
        reason: nova_files_api::SearchFailed::IndexFailed,
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
