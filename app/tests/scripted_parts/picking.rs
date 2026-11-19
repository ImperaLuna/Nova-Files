use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use gpui::{Context, FocusHandle, Focusable as _};
use nova_files_api::{Entry, Listing, ListingId, Order, SortKey};
use nova_files_shell::{Host, Opening, Part, Parts, Slots};

use super::super::*;

const SKIPPED: &str = "0";
const DIMMED: &str = "2";
const OPENING: u32 = 7;

thread_local! {
    static ONE: Cell<bool> = const { Cell::new(false) };
    static FOCUSED: Cell<bool> = const { Cell::new(false) };
    static REFUSED: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
    static REORDERED: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
}

struct Chooser {
    opened_with: Option<u32>,
    focus: FocusHandle,
}

fn listed(_: &mut Chooser, _: &Listing, reordered: bool, _: &mut Context<Chooser>) {
    REORDERED.with_borrow_mut(|seen| seen.push(reordered));
}

fn refused(_: &mut Chooser, _: ListingId, path: &Path, _: &mut Context<Chooser>) {
    REFUSED.with_borrow_mut(|refused| refused.push(path.to_owned()));
}

impl Part for Chooser {
    fn open(host: &Host, cx: &mut Context<Self>) -> Self {
        Self {
            opened_with: host.opening.get::<u32>().copied(),
            focus: cx.focus_handle(),
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.selectable(|_, entry: &Entry, _| entry.name != SKIPPED);
        slots.several(|_, _| !ONE.get());
        slots.dims(|_, entry, _| entry.name == DIMMED);
        slots.focus(|chooser, _| FOCUSED.get().then(|| chooser.focus.clone()));
        slots.refused(refused);
        slots.listed(listed);
    }
}

fn set_parts(cx: &TestAppContext, one: bool) {
    ONE.set(one);
    FOCUSED.set(false);
    REFUSED.with_borrow_mut(Vec::clear);
    REORDERED.with_borrow_mut(Vec::clear);
    let mut parts = Parts::default();
    parts.add::<Chooser>();
    cx.update(|cx| {
        cx.set_global(parts);
        nova_files_ui::bind_window_keys(cx);
    });
}

fn chosen(
    cx: &mut TestAppContext,
    one: bool,
) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    set_parts(cx, one);
    let (view, cx, mut core) = window_with_parts(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    (view, cx, core)
}

fn chooser(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Entity<Chooser> {
    view.read_with(cx, |view, _| view.part::<Chooser>().unwrap())
}

#[gpui::test]
fn a_part_keeps_entries_it_refuses_out_of_the_selection(cx: &mut TestAppContext) {
    let (view, cx, _core) = chosen(cx, false);
    cx.simulate_keystrokes("ctrl-a");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_paths()),
        [Path::new(SHOWN).join("1"), Path::new(SHOWN).join("2")]
    );
}

#[gpui::test]
fn a_part_that_wants_one_entry_keeps_the_selection_to_one(cx: &mut TestAppContext) {
    let (view, cx, _core) = chosen(cx, true);
    cx.simulate_keystrokes("ctrl-a");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.selected_paths()).len(), 1);
}

#[gpui::test]
fn a_part_dims_the_entries_it_names(cx: &mut TestAppContext) {
    let (view, cx, _core) = chosen(cx, false);
    view.read_with(cx, |view, cx| {
        assert!(view.dims(2, cx));
        assert!(!view.dims(1, cx));
    });
}

#[gpui::test]
fn a_part_focus_handle_takes_the_keys(cx: &mut TestAppContext) {
    let (view, cx, _core) = chosen(cx, false);
    view.read_with(cx, |view, cx| {
        assert_eq!(view.keys_home(cx), view.focus_handle(cx));
    });
    FOCUSED.set(true);
    let focus = chooser(&view, cx).read_with(cx, |chooser, _| chooser.focus.clone());
    view.read_with(cx, |view, cx| assert_eq!(view.keys_home(cx), focus));
}

#[gpui::test]
fn a_part_hears_a_refused_folder(cx: &mut TestAppContext) {
    let (view, cx, mut core) = chosen(cx, false);
    view.update(cx, |view, _| view.open(Path::new(MISSING)));
    let (id, path) = core.next_listing().unwrap();
    core.refuse(id, &path);
    cx.run_until_parked();
    REFUSED.with_borrow(|refused| assert_eq!(refused, &[PathBuf::from(MISSING)]));
}

#[gpui::test]
fn a_part_hears_whether_a_listing_only_changed_order(cx: &mut TestAppContext) {
    let (view, cx, mut core) = chosen(cx, false);
    view.update(cx, |view, _| view.sort_by(SortKey::Size));
    let (id, path) = core.next_listing().unwrap();
    core.send(Event::FolderListed(Listing {
        id,
        path,
        entries: Arc::from([]),
        folders: None,
    }));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.shown_order(),
            Order {
                key: SortKey::Size,
                descending: true,
            }
        );
    });
    REORDERED.with_borrow(|seen| assert_eq!(seen, &[false, true]));
}

#[gpui::test]
fn the_entries_say_which_listing_was_asked_for_last(cx: &mut TestAppContext) {
    let (view, cx, mut core) = chosen(cx, false);
    view.update(cx, |view, cx| {
        view.open(Path::new(OTHER));
        cx.notify();
    });
    let (id, _) = core.next_listing().unwrap();
    cx.run_until_parked();
    let entries = view.read_with(cx, |view, _| view.host().entries.clone());
    assert_eq!(
        entries.read_with(cx, |entries, _| entries.asked()),
        Some(id)
    );
}

#[gpui::test]
fn a_part_gets_what_its_window_was_opened_with(cx: &mut TestAppContext) {
    set_parts(cx, false);
    let (connector, _messages) = nova_files_api::channel();
    let (view, cx) = cx.add_window_view(|_, cx| {
        let location = nova_files_ui::Location::Folder(PathBuf::from(SHOWN));
        let opening = Opening::of(OPENING);
        MainWindow::opened_with(&connector, &location, PathBuf::from(SHOWN), opening, cx)
    });
    let opened_with = chooser(&view, cx).read_with(cx, |chooser, _| chooser.opened_with);
    assert_eq!(opened_with, Some(OPENING));
    assert!(view.read_with(cx, |view, cx| view.scene(cx).picker));
}

#[gpui::test]
fn a_window_opened_plainly_is_no_picker(cx: &mut TestAppContext) {
    let (view, cx, _core) = chosen(cx, false);
    let opened_with = chooser(&view, cx).read_with(cx, |chooser, _| chooser.opened_with);
    assert_eq!(opened_with, None);
    assert!(!view.read_with(cx, |view, cx| view.scene(cx).picker));
}

#[gpui::test]
fn a_part_closes_its_window(cx: &mut TestAppContext) {
    let (view, cx, _core) = chosen(cx, false);
    let nav = view.read_with(cx, |view, _| view.host().nav.clone());
    let app: &mut TestAppContext = cx;
    let windows = app.update(|cx| cx.windows().len());
    nav.update(app, nova_files_shell::Navigator::close);
    cx.run_until_parked();
    let app: &mut TestAppContext = cx;
    assert_eq!(app.update(|cx| cx.windows().len()), windows - 1);
}
