use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement as _, Modifiers, MouseButton,
    ParentElement as _, SharedString, Styled as _, Window, div, point, px,
};
use nova_files_api::{Entry, Listing};
use nova_files_open_api as open_api;
use nova_files_open_ui::OpenPart;
use nova_files_shell::{
    DropOver, Host, Location, NavEvent, NoteGone, PageKind, PageRef, Part, Parts, RowItem,
    SidebarAt, SidebarRow, Slots, Undo, assets::Icon,
};

use super::super::*;

const PICKED: &str = "2";
const TAGGED: &str = "1";

thread_local! {
    static DROPPED: Cell<usize> = const { Cell::new(0) };
    static OPENED: Cell<usize> = const { Cell::new(0) };
    static CLOSED: Cell<usize> = const { Cell::new(0) };
    static UNDONE: Cell<usize> = const { Cell::new(0) };
    static FILED: Cell<usize> = const { Cell::new(0) };
    static SWEPT: Cell<usize> = const { Cell::new(0) };
    static AIMED_AT: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

const FILE_ROOM: &str = "/file-room";

const OWN: &str = "0";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Basket;

impl PageKind for Basket {
    fn title(&self) -> SharedString {
        "Basket".into()
    }
}

struct Tagger {
    host: Host,
}

impl Tagger {
    fn listed(&mut self, listing: &Listing, _: bool, cx: &mut Context<Self>) {
        if listing.path == Path::new(SHOWN) {
            self.host
                .entries
                .update(cx, |entries, cx| entries.select(vec![PICKED.into()], cx));
        }
    }
}

fn shown(selector: &'static str) -> AnyElement {
    div()
        .debug_selector(move || selector.to_owned())
        .child(selector)
        .into_any_element()
}

fn opens(_: &mut Tagger, entry: &Entry, _: &Path, _: &mut Context<Tagger>) -> bool {
    let own = entry.name == OWN;
    if own {
        OPENED.set(OPENED.get() + 1);
    }
    own
}

fn files_here(_: &Tagger, over: &DropOver<'_>, _: &gpui::App) -> Option<SharedString> {
    let here = over.at.folder() == Some(Path::new(FILE_ROOM));
    if here {
        AIMED_AT.set(over.name.to_owned());
    }
    here.then(|| format!("File into {}", over.name).into())
}

fn basket(_: &mut Tagger, _: &Basket, _: &mut Window, _: &mut Context<Tagger>) -> AnyElement {
    div().child(DROPPED.get().to_string()).into_any_element()
}

impl Part for Tagger {
    fn open(host: &Host, _: &mut Context<Self>) -> Self {
        Self { host: host.clone() }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.name_cell(|_, entry, _| {
            (entry.name == TAGGED).then(|| {
                div()
                    .debug_selector(|| "tagged".to_owned())
                    .child("tagged")
                    .into_any_element()
            })
        });
        slots.listed(Self::listed);
        slots.bar(|_, _, _, _| Some(shown("tagger-bar")));
        slots.overlay(|_, scene, _, _| {
            scene
                .in_folder(Path::new(SHOWN))
                .then(|| shown("tagger-overlay"))
        });
        slots.list_view(
            |_, scene, _| scene.in_folder(Path::new(OTHER)),
            |_, _, _, _| Some(shown("tagger-list")),
        );
        slots.row_icon(|_, entry, side, _| {
            (entry.name == OWN).then(|| {
                div()
                    .debug_selector(|| "tagger-icon".to_owned())
                    .size(side)
                    .into_any_element()
            })
        });
        slots.opens(opens);
        slots.closing(|_, _| CLOSED.set(CLOSED.get() + 1));
        slots.page::<Basket>(basket);
        slots.drops::<Basket>(|_, _, paths, _| DROPPED.set(DROPPED.get() + paths.len()));
        slots.row_menu(|_, row, _| {
            if row == "place-file-room" {
                vec![vec![RowItem::new(Icon::Info, "Sweep", |_| {
                    SWEPT.set(SWEPT.get() + 1);
                })]]
            } else {
                Vec::new()
            }
        });
        slots.drop_target(files_here, |_, _, paths, _, _| {
            FILED.set(FILED.get() + paths.len());
        });
        slots.sidebar(SidebarAt::Places, |_, _, _| {
            vec![
                SidebarRow::new(
                    "place-basket",
                    Icon::Info,
                    "Basket",
                    Location::Page(PageRef::new(Basket)),
                ),
                SidebarRow::new(
                    "place-file-room",
                    Icon::Info,
                    "File room",
                    Location::Folder(FILE_ROOM.into()),
                ),
            ]
        });
    }
}

fn tagged(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    DROPPED.set(0);
    OPENED.set(0);
    CLOSED.set(0);
    UNDONE.set(0);
    FILED.set(0);
    SWEPT.set(0);
    let mut parts = Parts::default();
    parts.add::<Tagger>();
    parts.add::<OpenPart>();
    cx.update(|cx| {
        cx.set_global(parts);
        nova_files_ui::bind_window_keys(cx);
    });
    let (view, cx, mut core) = window_with_parts(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    (view, cx, core)
}

#[gpui::test]
fn a_part_can_draw_a_row_name_in_place_of_the_window(cx: &mut TestAppContext) {
    let (_view, cx, _core) = tagged(cx);
    assert!(cx.debug_bounds("tagged").is_some());
    assert!(cx.debug_bounds("name-0").is_some());
    assert!(cx.debug_bounds("name-1").is_none());
}

#[gpui::test]
fn a_part_selects_entries_once_their_listing_arrives(cx: &mut TestAppContext) {
    let (view, cx, _core) = tagged(cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_paths()),
        [Path::new(SHOWN).join(PICKED)]
    );
}

fn drag_onto(cx: &mut VisualTestContext, target: &'static str) {
    let from = cx.debug_bounds("name-0").unwrap().center();
    let to = cx.debug_bounds(target).unwrap().center();
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        from + point(px(20.0), px(0.0)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    cx.run_until_parked();
}

fn let_go(cx: &mut VisualTestContext, target: &'static str) {
    let to = cx.debug_bounds(target).unwrap().center();
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn rows_dropped_on_a_part_page_row_go_to_the_part(cx: &mut TestAppContext) {
    let (_view, cx, _core) = tagged(cx);
    drag_onto(cx, "place-basket");
    let_go(cx, "place-basket");
    assert_eq!(DROPPED.get(), 1);
}

#[gpui::test]
fn a_part_that_aims_at_a_folder_names_and_takes_the_drop(cx: &mut TestAppContext) {
    let (_view, cx, mut core) = tagged(cx);
    core.commands();
    drag_onto(cx, "place-file-room");
    let chip = cx.debug_bounds("drag-chip").is_some();
    let_go(cx, "place-file-room");

    assert!(chip);
    assert_eq!(AIMED_AT.take(), "File room");
    assert_eq!(FILED.get(), 1);
    assert_eq!(
        core.commands(),
        Vec::new(),
        "the window starts no job of its own"
    );
}

#[gpui::test]
fn a_part_fills_the_bar_the_overlay_and_a_row_icon(cx: &mut TestAppContext) {
    let (_view, cx, _core) = tagged(cx);
    assert!(cx.debug_bounds("tagger-bar").is_some());
    assert!(cx.debug_bounds("tagger-overlay").is_some());
    let icon = cx.debug_bounds("tagger-icon").unwrap();
    assert_eq!(icon.size.width, nova_files_shell::theme::size::ROW_ICON);
    assert!(cx.debug_bounds("tagger-list").is_none());
}

#[gpui::test]
fn a_part_list_view_draws_in_place_of_the_rows_where_it_applies(cx: &mut TestAppContext) {
    let (view, cx, mut core) = tagged(cx);
    view.update(cx, |view, _| view.open(Path::new(OTHER)));
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    assert!(cx.debug_bounds("tagger-list").is_some());
    assert!(cx.debug_bounds("name-0").is_none());
    assert!(cx.debug_bounds("tagger-overlay").is_none());
}

#[gpui::test]
fn a_part_opens_its_own_entries_and_the_rest_open_as_before(cx: &mut TestAppContext) {
    let (_view, cx, mut core) = tagged(cx);
    core.commands();
    cx.simulate_keystrokes("home shift-down enter");
    cx.run_until_parked();
    let opened: Vec<PathBuf> = core
        .sent::<open_api::Command>()
        .into_iter()
        .filter_map(|command| match command {
            open_api::Command::OpenFile { path, .. } => Some(path),
            open_api::Command::OpenWith(_) => None,
        })
        .collect();
    assert_eq!(OPENED.get(), 1);
    assert_eq!(opened, [Path::new(SHOWN).join("1")]);
}

#[gpui::test]
fn the_navigator_reveals_a_path_in_its_folder(cx: &mut TestAppContext) {
    let (view, cx, mut core) = tagged(cx);
    let target = Path::new(OTHER).join("1");
    let nav = view.read_with(cx, |view, _| view.host().nav.clone());
    nav.update(cx, |nav, cx| nav.reveal(target.clone(), cx));
    cx.run_until_parked();
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.selected_paths()),
        [target]
    );
}

#[gpui::test]
fn a_part_hears_its_window_closing(cx: &mut TestAppContext) {
    let (view, cx, _core) = tagged(cx);
    drop(view);
    cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    assert_eq!(CLOSED.get(), 1);
}

#[gpui::test]
fn undo_on_a_part_note_runs_the_part_undo(cx: &mut TestAppContext) {
    let (view, cx, _core) = tagged(cx);
    let island = view.read_with(cx, |view, _| view.host().island.clone());
    let undo = Undo::Run(Rc::new(|_| UNDONE.set(UNDONE.get() + 1)));
    island.update(cx, |island, cx| island.note("Tagged", Some(undo), cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert_eq!(UNDONE.get(), 1);
}

#[gpui::test]
fn a_part_row_menu_runs_the_chosen_item(cx: &mut TestAppContext) {
    let (view, cx, _core) = tagged(cx);
    let basket = cx.debug_bounds("place-basket").unwrap().center();
    cx.simulate_mouse_down(basket, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.menu().is_none()));

    let room = cx.debug_bounds("place-file-room").unwrap().center();
    cx.simulate_mouse_down(room, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();

    assert_eq!(SWEPT.get(), 1);
}

#[gpui::test]
fn a_part_hears_when_its_note_leaves_the_island(cx: &mut TestAppContext) {
    let (view, cx, _core) = tagged(cx);
    let island = view.read_with(cx, |view, _| view.host().island.clone());
    let gone = Rc::new(Cell::new(None));
    let heard = Rc::clone(&gone);
    let _hearing = cx.update(|_, cx| {
        cx.subscribe(&island, move |_, NoteGone(id): &NoteGone, _| {
            heard.set(Some(*id));
        })
    });
    let mine = island.update(cx, |island, cx| island.note("Tagged", None, cx));
    cx.run_until_parked();
    assert_eq!(gone.get(), None);

    view.update(cx, |view, cx| view.show_note("Something else", None, cx));
    cx.run_until_parked();

    assert_eq!(gone.get(), Some(mine));
}

#[gpui::test]
fn a_part_hears_when_the_window_goes_elsewhere_but_not_for_its_own_asks(cx: &mut TestAppContext) {
    let (view, cx, _core) = tagged(cx);
    let nav = view.read_with(cx, |view, _| view.host().nav.clone());
    let left = Rc::new(Cell::new(0));
    let heard = Rc::clone(&left);
    let _hearing = cx.update(|_, cx| {
        cx.subscribe(&nav, move |_, event: &NavEvent, _| {
            if *event == NavEvent::Left {
                heard.set(heard.get() + 1);
            }
        })
    });

    nav.update(cx, |nav, cx| nav.go(Location::Folder(FILE_ROOM.into()), cx));
    cx.run_until_parked();
    assert_eq!(left.get(), 0);

    view.update(cx, |view, cx| {
        view.open(Path::new(SHOWN));
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(left.get(), 1);
}
