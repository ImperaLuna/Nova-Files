use std::cell::Cell;

use gpui::{Context, WeakEntity};
use nova_files_shell::{
    Host, Location, Part, Parts, RowItem, SidebarAt, SidebarRow, Slots, assets::Icon,
};

use super::super::*;
use super::click;

const SHELF: &str = "/shelf";
const BOOKS: &str = "/shelf/books";

thread_local! {
    static EJECTED: Cell<usize> = const { Cell::new(0) };
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

impl Part for Rower {
    fn open(_: &Host, cx: &mut Context<Self>) -> Self {
        Self {
            me: cx.entity().downgrade(),
            open: false,
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.sidebar(SidebarAt::Drives, |_, _, _| {
            vec![
                SidebarRow::new(
                    "drive-cloud",
                    Icon::Drive,
                    "Cloud",
                    Location::Folder(SHELF.into()),
                )
                .button(RowItem::new(Icon::Eject, "Eject", |_| {
                    EJECTED.set(EJECTED.get() + 1);
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
    EJECTED.set(0);
    let mut parts = Parts::default();
    parts.add::<Rower>();
    cx.update(|cx| cx.set_global(parts));
    let (_view, cx, mut core) = window_with_parts(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    cx
}

#[gpui::test]
fn a_part_row_button_runs_without_opening_the_row(cx: &mut TestAppContext) {
    let cx = rowed(cx);
    assert!(cx.debug_bounds("drive-cloud").is_some());
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
