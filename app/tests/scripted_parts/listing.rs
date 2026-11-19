use gpui::{Context, SharedString};
use nova_files_api::{Command, Envelope, ListingId, Order};
use nova_files_shell::{Host, PageKind, PageRef, Part, Parts, Slots};
use nova_files_ui::Location;

use super::super::*;

const PILE: &str = "/pile";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Pile;

impl PageKind for Pile {
    fn title(&self) -> SharedString {
        "Pile".into()
    }
}

struct Piler;

impl Part for Piler {
    fn open(_: &Host, _: &mut Context<Self>) -> Self {
        Self
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.listing_page::<Pile>(|_, id: ListingId, order: Order| {
            Envelope::new(Command::ListFolder {
                id,
                path: PILE.into(),
                order,
            })
        });
    }
}

fn piled(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    let mut parts = Parts::default();
    parts.add::<Piler>();
    cx.update(|cx| cx.set_global(parts));
    let (view, cx, mut core) = window_with_parts(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    (view, cx, core)
}

#[gpui::test]
fn a_part_page_shows_the_listing_its_command_asks_for(cx: &mut TestAppContext) {
    let (view, cx, mut core) = piled(cx);
    let page = PageRef::new(Pile);
    view.update(cx, |view, cx| view.open_page(page.clone(), cx));
    cx.run_until_parked();
    let (id, asked) = core.next_listing().unwrap();
    assert_eq!(asked, Path::new(PILE));
    core.send(Event::FolderListed(Listing {
        id,
        path: asked,
        entries: (0..2)
            .map(|index| Entry {
                id: EntryId(index),
                name: format!("{index}").into(),
                kind: EntryKind::File,
            })
            .collect(),
        folders: None,
    }));
    cx.run_until_parked();

    view.read_with(cx, |view, _| {
        assert_eq!(view.location(), Some(&Location::Page(page.clone())));
        assert_eq!(view.listed_page(), Some(&page));
        assert_eq!(view.folder(), None);
        assert!(view.page_shown().is_none());
    });
    assert!(cx.debug_bounds("name-1").is_some());
    assert!(cx.debug_bounds("name-2").is_none());

    view.update(cx, |view, _| view.open(Path::new(SHOWN)));
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.listed_page(), None);
        assert_eq!(view.folder(), Some(Path::new(SHOWN)));
    });
}

#[gpui::test]
fn a_part_replaces_the_place_shown_without_a_step_back(cx: &mut TestAppContext) {
    let (view, cx, mut core) = piled(cx);
    let nav = view.read_with(cx, |view, _| view.host().nav.clone());
    nav.update(cx, |nav, cx| {
        nav.replace(nova_files_shell::Location::Page(PageRef::new(Pile)), cx);
    });
    cx.run_until_parked();
    answer_listing(&mut core, 2);
    cx.run_until_parked();

    view.read_with(cx, |view, _| {
        assert_eq!(view.listed_page(), Some(&PageRef::new(Pile)));
        assert_eq!(view.back_target(), None);
    });
}
