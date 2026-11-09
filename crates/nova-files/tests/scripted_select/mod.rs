//! Selecting against a scripted core: a fresh read of the same folder
//! keeps the names selected, Enter opens the files selected, and a page
//! move steps by the rows on screen.

use super::*;

/// Answers the window's next listing request with files named `names`.
fn answer_names(core: &mut Core, names: &[&str]) {
    let (id, path) = core.next_listing().unwrap();
    let entries: Arc<[Entry]> = names
        .iter()
        .zip(0..)
        .map(|(name, index)| Entry {
            id: EntryId(index),
            name: (*name).into(),
            kind: EntryKind::File,
        })
        .collect();
    core.send(Event::FolderListed(Listing { id, path, entries }));
}

/// A window on [`SHOWN`] with its keys bound, showing files `names`.
fn keyed<'a>(
    cx: &'a mut TestAppContext,
    names: &[&str],
) -> (Entity<MainWindow>, &'a mut VisualTestContext, Core) {
    cx.update(nova_files_ui::bind_window_keys);
    let (view, cx, mut core) = window(cx);
    answer_names(&mut core, names);
    cx.run_until_parked();
    (view, cx, core)
}

fn selected(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, _| view.selected_names())
}

/// The files the window asked the core to open since last asked.
fn opened(core: &Core) -> Vec<PathBuf> {
    let mut opened = Vec::new();
    while let Ok(message) = core.messages.try_recv() {
        if let CoreMessage::Command {
            command: Command::OpenFile { path, .. },
            ..
        } = message
        {
            opened.push(path);
        }
    }
    opened
}

#[gpui::test]
fn a_fresh_read_keeps_the_names_selected(cx: &mut TestAppContext) {
    let (view, cx, mut core) = keyed(cx, &["a", "b", "c", "d"]);
    cx.simulate_keystrokes("down down shift-down");
    assert_eq!(selected(&view, cx), ["b", "c"]);

    view.update(cx, |view, _| view.open(Path::new(SHOWN)));
    answer_names(&mut core, &["0", "a", "b", "c"]);
    cx.run_until_parked();
    assert_eq!(selected(&view, cx), ["b", "c"]);
    view.read_with(cx, |view, _| assert_eq!(view.cursor(), Some(3)));
}

#[gpui::test]
fn a_cursor_whose_file_went_lands_on_the_nearest_row(cx: &mut TestAppContext) {
    let (view, cx, mut core) = keyed(cx, &["a", "b", "c", "d"]);
    cx.simulate_keystrokes("end");

    view.update(cx, |view, _| view.open(Path::new(SHOWN)));
    answer_names(&mut core, &["a", "b"]);
    cx.run_until_parked();
    assert_eq!(selected(&view, cx), Vec::<String>::new());
    view.read_with(cx, |view, _| assert_eq!(view.cursor(), Some(1)));
    cx.simulate_keystrokes("up");
    assert_eq!(selected(&view, cx), ["a"]);
}

#[gpui::test]
fn enter_opens_the_files_selected(cx: &mut TestAppContext) {
    let (view, cx, core) = keyed(cx, &["a", "b", "c"]);
    opened(&core);
    cx.simulate_keystrokes("down shift-down enter");
    let shown = Path::new(SHOWN);
    assert_eq!(opened(&core), [shown.join("a"), shown.join("b")]);
    view.read_with(cx, |view, _| assert_eq!(view.folder(), Some(shown)));
}

#[gpui::test]
fn page_keys_step_by_the_rows_on_screen(cx: &mut TestAppContext) {
    let names: Vec<String> = (0..MANY).map(|index| format!("{index:03}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let (view, cx, _core) = keyed(cx, &names);
    let on_screen = view.read_with(cx, |view, _| view.rows_on_screen());
    assert!(on_screen > 1 && on_screen < names.len());

    cx.simulate_keystrokes("down pagedown");
    view.read_with(cx, |view, _| assert_eq!(view.cursor(), Some(on_screen)));
    cx.simulate_keystrokes("shift-pageup");
    assert_eq!(selected(&view, cx).len(), on_screen + 1);
    view.read_with(cx, |view, _| assert_eq!(view.cursor(), Some(0)));
}
