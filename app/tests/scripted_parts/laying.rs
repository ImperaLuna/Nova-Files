use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement as _, KeyDownEvent,
    ParentElement as _, Subscription, Window, div,
};
use nova_files_api::{Envelope, ListingId};
use nova_files_search_api::{Command as SearchCommand, Event as SearchEvent, SearchFailed};
use nova_files_shell::{
    ChipRank, Entries, Host, LaidOver, Location, Navigator, OverLeft, Part, Parts, Scene, Slots,
};

use super::super::*;

const FOUND: &str = "found";

thread_local! {
    static LEFT: Cell<usize> = const { Cell::new(0) };
}

struct Layer {
    host: Host,
    _left: Subscription,
}

fn ask(id: ListingId, from: &Location) -> Envelope {
    Envelope::new(SearchCommand::Find {
        id,
        query: FOUND.to_owned(),
        within: from.folder().map(Path::to_path_buf),
    })
}

fn chip(selector: &'static str) -> AnyElement {
    div()
        .debug_selector(move || selector.to_owned())
        .child(selector)
        .into_any_element()
}

impl Layer {
    fn key_down(&mut self, event: &KeyDownEvent, _: &Scene, cx: &mut Context<Self>) -> bool {
        match event.keystroke.key.as_str() {
            "l" => {
                let over = LaidOver {
                    ask: Rc::new(ask),
                    marks: FOUND.to_owned(),
                };
                self.host
                    .entries
                    .update(cx, |entries, cx| entries.lay_over(over, cx));
            }
            "b" => self.host.entries.update(cx, Entries::take_back),
            "r" => self.host.nav.update(cx, Navigator::focus_results),
            _ => return false,
        }
        true
    }

    fn failed(&self, event: &SearchEvent, cx: &mut Context<Self>) {
        if let SearchEvent::FindFailed { id, .. } = event {
            let id = *id;
            self.host
                .entries
                .update(cx, |entries, cx| entries.listing_failed(id, cx));
        }
    }
}

impl Part for Layer {
    fn open(host: &Host, cx: &mut Context<Self>) -> Self {
        let left = cx.subscribe(&host.entries, |_, _, _: &OverLeft, _| {
            LEFT.set(LEFT.get() + 1);
        });
        Self {
            host: host.clone(),
            _left: left,
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.key_down(Self::key_down);
        slots.on_event(|layer: &mut Self, event: SearchEvent, cx| layer.failed(&event, cx));
        slots.path_chip(|_, _, _, _| Some(chip("later-chip")));
        slots.path_chip_at(ChipRank::FIRST, |_, _, _, _| Some(chip("first-chip")));
        slots.overlay(|layer: &mut Self, _, _: &mut Window, cx| {
            layer
                .host
                .entries
                .read(cx)
                .over_failed()
                .then(|| chip("laid-failed"))
        });
    }
}

fn laying(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    LEFT.set(0);
    let mut parts = Parts::default();
    parts.add::<Layer>();
    cx.update(|cx| cx.set_global(parts));
    let (view, cx, mut core) = window_with_parts(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    (view, cx, core)
}

fn asked(core: &mut Core) -> Option<(ListingId, Option<PathBuf>)> {
    core.sent::<SearchCommand>()
        .into_iter()
        .filter_map(|command| match command {
            SearchCommand::Find { id, within, .. } => Some((id, within)),
            SearchCommand::Search { .. } => None,
        })
        .next_back()
}

fn answer_found(core: &Core, id: ListingId) {
    core.send(Event::FolderListed(Listing {
        id,
        path: PathBuf::from(SHOWN),
        entries: Arc::new([Entry {
            id: EntryId(0),
            name: FOUND.into(),
            kind: EntryKind::File,
        }]),
        folders: Some(Arc::new([PathBuf::from(SHOWN)])),
    }));
}

fn laid_over(cx: &mut VisualTestContext, core: &mut Core) -> ListingId {
    cx.simulate_keystrokes("l");
    cx.run_until_parked();
    let (id, within) = asked(core).unwrap();
    assert_eq!(within, Some(PathBuf::from(SHOWN)));
    id
}

#[gpui::test]
fn a_part_lays_a_listing_over_the_folder_and_takes_it_back(cx: &mut TestAppContext) {
    let (view, cx, mut core) = laying(cx);
    let id = laid_over(cx, &mut core);
    answer_found(&core, id);
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.searching());
        assert_eq!(view.entry_names(), [FOUND]);
        assert_eq!(view.folder(), Some(Path::new(SHOWN)));
    });

    cx.simulate_keystrokes("b");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |view, _| view.searching()));
    assert_eq!(
        core.next_listing().map(|(_, path)| path),
        Some(PathBuf::from(SHOWN))
    );
    assert_eq!(LEFT.get(), 0);
}

#[gpui::test]
fn going_elsewhere_leaves_the_laid_over_listing_and_tells_the_part(cx: &mut TestAppContext) {
    let (view, cx, mut core) = laying(cx);
    let id = laid_over(cx, &mut core);
    answer_found(&core, id);
    cx.run_until_parked();

    view.update(cx, |view, _| view.open(Path::new(OTHER)));
    cx.run_until_parked();
    answer_listing(&mut core, 1);
    cx.run_until_parked();
    assert!(!view.read_with(cx, |view, _| view.searching()));
    assert_eq!(LEFT.get(), 1);
}

#[gpui::test]
fn a_failed_ask_shows_nothing_over_the_folder_and_says_it_failed(cx: &mut TestAppContext) {
    let (view, cx, mut core) = laying(cx);
    let id = laid_over(cx, &mut core);
    assert!(cx.debug_bounds("laid-failed").is_none());
    core.send(SearchEvent::FindFailed {
        id,
        reason: SearchFailed::NoIndex,
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.searching());
        assert!(view.entry_names().is_empty());
    });
    assert!(cx.debug_bounds("laid-failed").is_some());
}

#[gpui::test]
fn a_part_can_put_the_cursor_on_the_first_result(cx: &mut TestAppContext) {
    let (view, cx, _core) = laying(cx);
    assert_eq!(view.read_with(cx, |view, _| view.cursor()), None);
    cx.simulate_keystrokes("r");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.cursor()), Some(0));
}

#[gpui::test]
fn a_first_ranked_path_chip_comes_before_the_others(cx: &mut TestAppContext) {
    let (_view, cx, _core) = laying(cx);
    let first = cx.debug_bounds("first-chip").unwrap();
    let later = cx.debug_bounds("later-chip").unwrap();
    assert!(first.left() < later.left());
}
