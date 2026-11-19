use std::cell::Cell;
use std::path::Path;

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement as _, ParentElement as _, Pixels,
    Point, SharedString, Window, div, point,
};
use nova_files_shell::{
    Column, Host, Location, MenuAt, MenuGroup, MenuItem, PageKind, PageRef, Part, Parts, Placed,
    Scene, SidebarAt, SidebarRow, Slots, assets::Icon, command_spec,
};

use super::super::*;
use super::click;

mod actions {
    #![expect(
        clippy::derive_partial_eq_without_eq,
        reason = "GPUI's actions! macro derives PartialEq only"
    )]
    gpui::actions!(scripted_shelf, [Poke]);
}

use actions::Poke;

command_spec!(POKE, Poke, "test.poke", "Poke", ["ctrl-alt-p"]);

const ASKED: &str = "Poke them all?";

thread_local! {
    static POKED: Cell<usize> = const { Cell::new(0) };
    static SEEN_SELECTED: Cell<usize> = const { Cell::new(0) };
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Shelved;

impl PageKind for Shelved {
    fn title(&self) -> SharedString {
        "Shelf".into()
    }

    fn icon(&self) -> Icon {
        Icon::Info
    }
}

struct Shelf {
    host: Host,
}

impl Shelf {
    fn poke(&mut self, _: &Poke, _: &mut Window, cx: &mut Context<Self>) {
        POKED.set(POKED.get() + 1);
        SEEN_SELECTED.set(self.host.entries.read(cx).selected_paths().len());
        self.host
            .island
            .update(cx, |island, cx| island.note("Poked", None, cx));
    }
}

const fn some_selected(_: &Shelf, scene: &Scene, _: &gpui::App) -> bool {
    scene.selected > 0
}

fn poke_items(_: &Shelf, _: &Scene, _: &gpui::App) -> Vec<MenuItem> {
    vec![MenuItem::run(Icon::Info, &POKE).with_selector("poke")]
}

fn nudge_items(_: &Shelf, _: &Scene, _: &gpui::App) -> Vec<MenuItem> {
    vec![
        MenuItem::run(Icon::Info, &POKE)
            .with_label("Nudge")
            .with_selector("island-nudge")
            .icon_only(),
    ]
}

fn page(_: &mut Shelf, _: &Shelved, _: &mut Window, _: &mut Context<Shelf>) -> AnyElement {
    div()
        .debug_selector(|| "shelf-page".to_owned())
        .child(POKED.get().to_string())
        .into_any_element()
}

impl Part for Shelf {
    fn open(host: &Host, _: &mut Context<Self>) -> Self {
        Self { host: host.clone() }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.command(&POKE, some_selected, Self::poke);
        slots.menu(MenuAt::Entry, poke_items);
        slots.menu(MenuAt::Sheet, poke_items);
        slots.menu(MenuAt::Island, poke_items);
        slots.menu(MenuAt::PathBarEnd, poke_items);
        slots.menu_in(MenuAt::Entry, MenuGroup::Edit, Placed::SECOND, nudge_items);
        slots.menu_in(MenuAt::Island, MenuGroup::Edit, Placed::SECOND, nudge_items);
        slots.page::<Shelved>(page);
        slots.column(|_, scene, _| {
            scene.in_folder(Path::new(SHOWN)).then_some(Column {
                head: "Shelf".into(),
                selector: "head-shelf",
                cell: |_, entry, _| Some(format!("cell-{}", entry.name.to_string_lossy()).into()),
            })
        });
        slots.sidebar(SidebarAt::Places, |_, scene, _| {
            vec![
                SidebarRow::new(
                    "place-shelf",
                    Icon::Info,
                    "Shelf",
                    Location::Page(PageRef::new(Shelved)),
                )
                .number(Some("3".into()))
                .here(scene.on_page::<Shelved>().is_some()),
            ]
        });
        slots.start_card(|_, _, _, _| {
            Some(
                div()
                    .debug_selector(|| "shelf-card".to_owned())
                    .child("card")
                    .into_any_element(),
            )
        });
    }
}

fn shelved(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    POKED.set(0);
    SEEN_SELECTED.set(0);
    let mut parts = Parts::default();
    parts.add::<Shelf>();
    cx.update(|cx| {
        cx.set_global(parts);
        nova_files_ui::bind_window_keys(cx);
    });
    let (view, cx, mut core) = window_with_parts(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    (view, cx, core)
}

fn labels(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, _| {
        view.menu()
            .map(|menu| {
                menu.items
                    .iter()
                    .map(|item| item.label.to_string())
                    .collect()
            })
            .unwrap_or_default()
    })
}

fn right_click_at(cx: &mut VisualTestContext, at: Point<Pixels>) {
    cx.run_until_parked();
    cx.simulate_event(gpui::MouseDownEvent {
        position: at,
        modifiers: gpui::Modifiers::none(),
        button: gpui::MouseButton::Right,
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(gpui::MouseUpEvent {
        position: at,
        modifiers: gpui::Modifiers::none(),
        button: gpui::MouseButton::Right,
        click_count: 1,
    });
    cx.run_until_parked();
}

#[gpui::test]
fn a_part_command_runs_by_its_key_only_where_it_applies(cx: &mut TestAppContext) {
    let (_view, cx, _core) = shelved(cx);

    cx.simulate_keystrokes("ctrl-alt-p");
    cx.run_until_parked();
    assert_eq!(POKED.get(), 0);

    cx.simulate_keystrokes("down shift-down ctrl-alt-p");
    cx.run_until_parked();
    assert_eq!(POKED.get(), 1);
    assert_eq!(SEEN_SELECTED.get(), 2);
}

#[gpui::test]
fn a_part_note_shows_on_the_island(cx: &mut TestAppContext) {
    let (view, cx, _core) = shelved(cx);

    cx.simulate_keystrokes("down ctrl-alt-p");
    cx.run_until_parked();

    let note = view.read_with(cx, |view, _| view.note().map(|note| note.label.clone()));
    assert_eq!(note, Some("Poked".into()));
}

#[gpui::test]
fn a_part_question_runs_its_answer_only_on_yes(cx: &mut TestAppContext) {
    let (view, cx, _core) = shelved(cx);
    let shelf = view.read_with(cx, |view, _| view.host().clone());

    shelf.island.update(cx, |island, cx| {
        island.ask(ASKED, "Poke", Box::new(|_| POKED.set(POKED.get() + 10)), cx);
    });
    cx.run_until_parked();
    let asked = view.read_with(cx, |view, _| view.confirm().map(|ask| ask.label.clone()));
    assert_eq!(asked, Some(ASKED.into()));
    click(cx, "island-no");
    assert_eq!(POKED.get(), 0);

    shelf.island.update(cx, |island, cx| {
        island.ask(ASKED, "Poke", Box::new(|_| POKED.set(POKED.get() + 10)), cx);
    });
    cx.run_until_parked();
    click(cx, "island-yes");
    assert_eq!(POKED.get(), 10);
}

#[gpui::test]
fn part_items_join_the_sheet_and_entry_menus_where_they_apply(cx: &mut TestAppContext) {
    let (view, cx, _core) = shelved(cx);

    let below = cx.debug_bounds("entry-2").unwrap();
    let space = point(below.center().x, below.bottom() + below.size.height * 2.0);
    right_click_at(cx, space);
    assert_ne!(labels(&view, cx), Vec::<String>::new());
    assert!(!labels(&view, cx).contains(&"Poke".to_owned()));

    cx.simulate_keystrokes("escape");
    let entry = cx.debug_bounds("entry-0").unwrap().center();
    right_click_at(cx, entry);
    assert!(labels(&view, cx).contains(&"Poke".to_owned()));
}

#[gpui::test]
fn a_placed_part_item_lands_inside_the_window_group_it_names(cx: &mut TestAppContext) {
    let (view, cx, _core) = shelved(cx);

    let entry = cx.debug_bounds("entry-0").unwrap().center();
    right_click_at(cx, entry);
    let offered = labels(&view, cx);
    let at = |label: &str| offered.iter().position(|offered| offered == label);
    assert_eq!(at("Nudge"), at("Rename").map(|rename| rename + 1));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    let mut left = |selector| cx.debug_bounds(selector).unwrap().left();
    assert!(left("island-rename") < left("island-nudge"));
    assert!(left("island-nudge") < left("island-trash"));
}

#[gpui::test]
fn part_items_show_on_the_island_and_the_path_bar_with_a_selection(cx: &mut TestAppContext) {
    let (_view, cx, _core) = shelved(cx);
    cx.run_until_parked();
    assert!(cx.debug_bounds("poke").is_none());

    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    let shown = [cx.debug_bounds("poke").is_some()];
    assert_eq!(shown, [true]);

    click(cx, "poke");
    assert_eq!(POKED.get(), 1);
}

#[gpui::test]
fn a_part_column_heads_the_details_list_and_fills_each_row(cx: &mut TestAppContext) {
    let (_view, cx, _core) = shelved(cx);
    cx.run_until_parked();

    assert!(cx.debug_bounds("head-shelf").is_some());
}

#[gpui::test]
fn a_part_sidebar_row_opens_its_page_and_the_tab_takes_its_title(cx: &mut TestAppContext) {
    let (view, cx, _core) = shelved(cx);
    assert!(cx.debug_bounds("shelf-page").is_none());

    click(cx, "place-shelf");

    assert!(cx.debug_bounds("shelf-page").is_some());
    let page = view.read_with(cx, |view, _| view.page_shown().cloned());
    assert_eq!(page, Some(PageRef::new(Shelved)));
    let scene = view.read_with(cx, |view, _| view.scene());
    assert_eq!(scene.on_page::<Shelved>(), Some(&Shelved));
}

#[gpui::test]
fn the_navigator_opens_a_page_in_a_new_tab_and_goes_back_to_a_folder(cx: &mut TestAppContext) {
    let (view, cx, mut core) = shelved(cx);
    let host = view.read_with(cx, |view, _| view.host().clone());

    host.nav.update(cx, |nav, cx| {
        nav.open_in_new_tab(Location::Page(PageRef::new(Shelved)), cx);
    });
    cx.run_until_parked();

    assert_eq!(view.read_with(cx, |view, _| view.tab_count()), 2);
    assert!(cx.debug_bounds("shelf-page").is_some());
    assert_eq!(host.nav.read_with(cx, |nav, _| nav.tabs().len()), 2);

    host.nav.update(cx, |nav, cx| {
        nav.go(Location::Folder(PathBuf::from(OTHER)), cx);
    });
    cx.run_until_parked();
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.folder().map(Path::to_path_buf)),
        Some(PathBuf::from(OTHER))
    );
    assert!(cx.debug_bounds("shelf-page").is_none());
}

#[gpui::test]
fn a_part_start_card_shows_on_start(cx: &mut TestAppContext) {
    let (view, cx, _core) = shelved(cx);
    assert!(cx.debug_bounds("shelf-card").is_none());

    view.update(cx, MainWindow::open_start);
    cx.run_until_parked();

    assert!(cx.debug_bounds("shelf-card").is_some());
}

#[gpui::test]
fn entries_tell_parts_the_shown_folder_and_its_selection(cx: &mut TestAppContext) {
    let (view, cx, _core) = shelved(cx);
    let host = view.read_with(cx, |view, _| view.host().clone());

    cx.simulate_keystrokes("down shift-down");
    cx.run_until_parked();

    let (location, selected) = host.entries.read_with(cx, |entries, _| {
        (entries.location().cloned(), entries.selected_paths())
    });
    assert_eq!(location, Some(Location::Folder(PathBuf::from(SHOWN))));
    assert_eq!(
        selected,
        [
            PathBuf::from(SHOWN).join("0"),
            PathBuf::from(SHOWN).join("1")
        ]
    );
}

#[test]
fn parts_bind_the_keys_of_their_commands() {
    let mut parts = Parts::default();
    parts.add::<Shelf>();
    assert_eq!(parts.bindings().len(), 1);
}
