use gpui::{Context, SharedString};
use nova_files_api::{Command, Envelope, ListingId, Order};
use nova_files_shell::{Host, PageKind, PageRef, Part, Parts, Slots, Views};
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct Found;

impl PageKind for Found {
    fn title(&self) -> SharedString {
        "Found".into()
    }

    fn views(&self) -> Views {
        Views::DetailsOnly
    }

    fn folder_column(&self) -> bool {
        true
    }

    fn sorted(&self) -> bool {
        false
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
        slots.listing_page::<Found>(|_, id: ListingId, order: Order| {
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
    parts.add::<nova_files_folders_ui::FoldersPart>();
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
        nav.replace(Location::Page(PageRef::new(Pile)), cx);
    });
    cx.run_until_parked();
    answer_listing(&mut core, 2);
    cx.run_until_parked();

    view.read_with(cx, |view, _| {
        assert_eq!(view.listed_page(), Some(&PageRef::new(Pile)));
        assert_eq!(view.back_target(), None);
    });
}

#[gpui::test]
fn a_failed_listing_shows_the_page_empty(cx: &mut TestAppContext) {
    let (view, cx, mut core) = piled(cx);
    view.update(cx, |view, cx| view.open_page(PageRef::new(Pile), cx));
    cx.run_until_parked();
    let (id, _) = core.next_listing().unwrap();
    let entries = view.read_with(cx, |view, _| view.host().entries.clone());
    entries.update(cx, |entries, cx| entries.listing_failed(id, cx));
    cx.run_until_parked();

    view.read_with(cx, |view, _| {
        assert_eq!(view.listed_page(), Some(&PageRef::new(Pile)));
        assert_eq!(view.entry_names(), Vec::<String>::new());
        assert!(!view.waiting());
    });
}

#[gpui::test]
fn a_listed_page_can_keep_to_details_with_a_folder_column_and_no_sorting(cx: &mut TestAppContext) {
    cx.update(nova_files_ui::bind_window_keys);
    let (view, cx, mut core) = piled(cx);
    view.update(cx, |view, cx| view.open_page(PageRef::new(Found), cx));
    cx.run_until_parked();
    answer_listing(&mut core, 2);
    cx.run_until_parked();

    cx.simulate_keystrokes("ctrl-2");
    cx.run_until_parked();
    let view_mode = view.read_with(cx, MainWindow::folder_view);
    assert_eq!(view_mode, nova_files_folders_api::FolderView::default());
    assert!(cx.debug_bounds("folder-header").is_some());
    assert!(cx.debug_bounds("arrow-name").is_none());
}
