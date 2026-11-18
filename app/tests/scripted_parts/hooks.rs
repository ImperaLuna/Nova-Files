use std::cell::Cell;
use std::path::Path;

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement as _, Modifiers, MouseButton,
    ParentElement as _, SharedString, Window, div, point, px,
};
use nova_files_api::Listing;
use nova_files_shell::{
    Host, Location, PageKind, PageRef, Part, Parts, SidebarAt, SidebarRow, Slots, assets::Icon,
};

use super::super::*;

const PICKED: &str = "2";
const TAGGED: &str = "1";

thread_local! {
    static DROPPED: Cell<usize> = const { Cell::new(0) };
}

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
    fn listed(&mut self, listing: &Listing, cx: &mut Context<Self>) {
        if listing.path == Path::new(SHOWN) {
            self.host
                .entries
                .update(cx, |entries, cx| entries.select(vec![PICKED.into()], cx));
        }
    }
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
        slots.page::<Basket>(basket);
        slots.drops::<Basket>(|_, _, paths, _| DROPPED.set(DROPPED.get() + paths.len()));
        slots.sidebar(SidebarAt::Places, |_, _, _| {
            vec![SidebarRow {
                id: "place-basket",
                icon: Icon::Info,
                name: "Basket".into(),
                number: None,
                opens: Location::Page(PageRef::new(Basket)),
                here: false,
            }]
        });
    }
}

fn tagged(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    DROPPED.set(0);
    let mut parts = Parts::default();
    parts.add::<Tagger>();
    cx.update(|cx| cx.set_global(parts));
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

#[gpui::test]
fn rows_dropped_on_a_part_page_row_go_to_the_part(cx: &mut TestAppContext) {
    let (_view, cx, _core) = tagged(cx);
    let from = cx.debug_bounds("name-0").unwrap().center();
    let to = cx.debug_bounds("place-basket").unwrap().center();
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        from + point(px(20.0), px(0.0)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    cx.run_until_parked();
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(DROPPED.get(), 1);
}
