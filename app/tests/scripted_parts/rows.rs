use std::cell::Cell;

use gpui::{App, Context, Modifiers, MouseButton, WeakEntity};
use nova_files_picker_api::{PickId, PickKind, PickReply, PickRequest};
use nova_files_shell::{
    Host, KnownFolder, KnownFolders, Location, Look, Part, Parts, Published, RowItem, SidebarAt,
    SidebarRow, Slots, assets::Icon,
};

use super::super::*;
use super::click;

const SHELF: &str = "/shelf";
const BOOKS: &str = "/shelf/books";

thread_local! {
    static EJECTED: Cell<usize> = const { Cell::new(0) };
    static ADDED: Cell<usize> = const { Cell::new(0) };
    static MOUNTED: Cell<usize> = const { Cell::new(0) };
    static MOUNTED_IN_TAB: Cell<usize> = const { Cell::new(0) };
}

struct Rower {
    me: WeakEntity<Self>,
    open: bool,
}

impl Rower {
    fn toggle(&self) -> RowItem {
        let me = self.me.clone();
        RowItem::new(Icon::Chevron, "Open", move |cx| {
            me.update(cx, |rower, cx| {
                rower.open = !rower.open;
                cx.notify();
            })
            .ok();
        })
    }
}

fn row_menu(_: &Rower, row: &str, _: &App) -> Vec<Vec<RowItem>> {
    if row != "place-first" {
        return Vec::new();
    }
    vec![
        vec![
            RowItem::new(Icon::Info, "Look", |_| {}),
            RowItem::new(Icon::Info, "Look closer", |_| {}),
        ],
        Vec::new(),
        vec![RowItem::new(Icon::Info, "Forget", |_| {})],
    ]
}

impl Part for Rower {
    fn open(_: &Host, cx: &mut Context<Self>) -> Self {
        Self {
            me: cx.entity().downgrade(),
            open: false,
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.sidebar(SidebarAt::Places, |_, _, _| {
            vec![
                SidebarRow::new(
                    "place-first",
                    Icon::Folder,
                    "First",
                    Location::Folder(SHELF.into()),
                )
                .rank(SidebarRow::FIRST),
                SidebarRow::new(
                    "place-end",
                    Icon::Folder,
                    "End",
                    Location::Folder(BOOKS.into()),
                ),
            ]
        });
        slots.row_menu(row_menu);
        slots.section_button(SidebarAt::Places, "rows-add", |_, _, _| {
            Some(RowItem::new(Icon::Plus, "Add", |_| {
                ADDED.set(ADDED.get() + 1);
            }))
        });
        slots.sidebar(SidebarAt::Drives, |_, _, _| {
            vec![
                SidebarRow::new(
                    "drive-cloud",
                    Icon::Drive,
                    "Cloud",
                    Location::Folder(SHELF.into()),
                )
                .number(Some("1 GB free".into()))
                .number_look(Look::Danger)
                .button(RowItem::new(Icon::Eject, "Eject", |_| {
                    EJECTED.set(EJECTED.get() + 1);
                })),
                SidebarRow::acting(
                    "drive-off",
                    Icon::Drive,
                    "Off",
                    RowItem::new(Icon::Drive, "Mount", |_| {
                        MOUNTED.set(MOUNTED.get() + 1);
                    }),
                )
                .on_new_tab(RowItem::new(Icon::Drive, "Mount in a tab", |_| {
                    MOUNTED_IN_TAB.set(MOUNTED_IN_TAB.get() + 1);
                })),
            ]
        });
        slots.sidebar(SidebarAt::Tree, |rower, _, _| {
            let shelf = SidebarRow::new(
                "tree-shelf",
                Icon::Folder,
                "Shelf",
                Location::Folder(SHELF.into()),
            )
            .expander(rower.open, rower.toggle());
            let books = SidebarRow::new(
                "tree-books",
                Icon::Folder,
                "Books",
                Location::Folder(BOOKS.into()),
            )
            .depth(1);
            if rower.open {
                vec![shelf, books]
            } else {
                vec![shelf]
            }
        });
    }
}

fn rowed(cx: &mut TestAppContext) -> &mut VisualTestContext {
    rowed_window(cx).1
}

fn rowed_window(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext) {
    EJECTED.set(0);
    ADDED.set(0);
    MOUNTED.set(0);
    MOUNTED_IN_TAB.set(0);
    let mut parts = Parts::default();
    parts.add::<Rower>();
    cx.update(|cx| cx.set_global(parts));
    let (view, cx, mut core) = window_with_parts(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    (view, cx)
}

#[gpui::test]
fn a_part_row_button_runs_without_opening_the_row(cx: &mut TestAppContext) {
    let cx = rowed(cx);
    assert!(cx.debug_bounds("drive-cloud").is_some());
    assert!(cx.debug_bounds("drive-cloud-number").is_some());
    click(cx, "drive-cloud-button");
    assert_eq!(EJECTED.get(), 1);
}

#[gpui::test]
fn a_part_tree_row_opens_and_shows_its_children_indented(cx: &mut TestAppContext) {
    let cx = rowed(cx);
    assert!(cx.debug_bounds("tree-books").is_none());

    click(cx, "tree-shelf-expander");

    let shelf = cx.debug_bounds("tree-shelf").unwrap();
    let books = cx.debug_bounds("tree-books").unwrap();
    assert!(books.top() > shelf.top());
    assert!(cx.debug_bounds("tree-books-expander").is_none());

    click(cx, "tree-shelf-expander");
    assert!(cx.debug_bounds("tree-books").is_none());
}

#[gpui::test]
fn part_rows_take_their_place_by_rank_among_the_window_rows(cx: &mut TestAppContext) {
    let cx = rowed(cx);
    let top = |cx: &mut VisualTestContext, selector| cx.debug_bounds(selector).unwrap().top();
    assert!(top(cx, "place-first") < top(cx, "place-start"));
    assert!(top(cx, "place-end") > top(cx, "place-projects"));
    assert!(top(cx, "place-end") > top(cx, "place-games"));
}

#[gpui::test]
fn a_part_row_menu_opens_in_the_main_window(cx: &mut TestAppContext) {
    let (view, cx) = rowed_window(cx);
    let first = cx.debug_bounds("place-first").unwrap().center();
    cx.simulate_mouse_down(first, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    let lines = view.read_with(cx, |view, _| view.menu().map(|menu| menu.lines.clone()));
    assert_eq!(lines, Some(vec![2]));
}

#[gpui::test]
fn a_part_button_on_a_section_head_runs(cx: &mut TestAppContext) {
    let cx = rowed(cx);
    assert!(cx.debug_bounds("pin").is_some());
    click(cx, "rows-add");
    assert_eq!(ADDED.get(), 1);
}

#[gpui::test]
fn the_picker_shows_part_rows_without_their_menu_or_head_buttons(cx: &mut TestAppContext) {
    let (view, cx) = rowed_window(cx);
    let request = Box::new(PickRequest {
        id: PickId(7),
        kind: PickKind::Open {
            multiple: false,
            folders: true,
        },
        title: String::new(),
        accept_label: None,
        filters: Vec::new(),
        filter: None,
        folder: None,
        parent: None,
        types: None,
        reply: PickReply::new(|_| {}),
    });
    view.update_in(cx, |view, window, cx| view.set_pick(request, window, cx));
    cx.run_until_parked();

    assert!(cx.debug_bounds("place-first").is_some());
    assert!(cx.debug_bounds("drive-cloud").is_some());
    assert!(cx.debug_bounds("rows-add").is_none());
    let first = cx.debug_bounds("place-first").unwrap().center();
    cx.simulate_mouse_down(first, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.menu().is_none()));
}

#[gpui::test]
fn a_part_row_with_its_own_actions_runs_them_instead_of_opening(cx: &mut TestAppContext) {
    let (view, cx) = rowed_window(cx);
    let before = view.read_with(cx, |view, _| view.tab_count());

    click(cx, "drive-off");
    assert_eq!(MOUNTED.get(), 1);

    let off = cx.debug_bounds("drive-off").unwrap().center();
    cx.simulate_mouse_down(off, MouseButton::Middle, Modifiers::none());
    cx.simulate_mouse_up(off, MouseButton::Middle, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(MOUNTED_IN_TAB.get(), 1);
    assert_eq!(view.read_with(cx, |view, _| view.tab_count()), before);
}

#[gpui::test]
fn the_window_draws_folders_with_the_icons_a_part_publishes(cx: &mut TestAppContext) {
    let (view, cx) = rowed_window(cx);
    let known = KnownFolders::new([KnownFolder {
        path: BOOKS.into(),
        name: "Books".into(),
        icon: Icon::Documents,
    }]);
    cx.update(|_, cx| Published::set(known, cx));
    cx.run_until_parked();
    let icon = view.read_with(cx, |view, _| view.known_folders().icon_at(Path::new(BOOKS)));
    assert_eq!(icon, Icon::Documents);
}
