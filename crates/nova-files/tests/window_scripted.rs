//! The window against a scripted core: the test reads each command the
//! window sends and answers it by hand, so the order of events is exact.

#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use gpui::{Entity, Point, TestAppContext, VisualTestContext, point, px};
use nova_files_api::{
    Command, CoreMessage, Details, Entry, EntryId, EntryKind, Event, EventSink, Listing, ListingId,
    Unreadable,
};
use nova_files_ui::MainWindow;

const SHOWN: &str = "/shown";
const MISSING: &str = "/shown/missing";
const OTHER: &str = "/other";
const ENTRIES: u32 = 3;
/// Enough rows that the list scrolls in a test window.
const MANY: u32 = 500;

struct Core {
    messages: Receiver<CoreMessage>,
    events: Option<Box<dyn EventSink>>,
}

impl Core {
    /// The next folder the window asks for, skipping other commands.
    fn next_listing(&mut self) -> Option<(ListingId, PathBuf)> {
        while let Ok(message) = self.messages.try_recv() {
            match message {
                CoreMessage::Connected { events, .. } => self.events = Some(events),
                CoreMessage::Command {
                    command: Command::ListFolder { id, path },
                    ..
                } => return Some((id, path)),
                _ => {}
            }
        }
        None
    }

    fn send(&self, event: Event) {
        self.events.as_ref().unwrap().deliver(event).unwrap();
    }

    fn refuse(&self, id: ListingId, path: &Path) {
        self.send(Event::FolderUnreadable {
            id,
            path: path.to_owned(),
            reason: Unreadable::Missing,
        });
    }
}

/// A window on [`SHOWN`] and its scripted core; nothing answered yet.
fn window(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    let (connector, messages) = nova_files_api::channel();
    let (view, cx) = cx.add_window_view(|_, cx| {
        MainWindow::with_home(&connector, Path::new(SHOWN), PathBuf::from(SHOWN), cx)
    });
    let core = Core {
        messages,
        events: None,
    };
    (view, cx, core)
}

/// Answers the window's next listing request with `count` files.
fn answer_listing(core: &mut Core, count: u32) -> ListingId {
    let (id, path) = core.next_listing().unwrap();
    let entries: Arc<[Entry]> = (0..count)
        .map(|index| Entry {
            id: EntryId(index),
            name: format!("{index}").into(),
            kind: EntryKind::File,
        })
        .collect();
    core.send(Event::FolderListed(Listing { id, path, entries }));
    id
}

/// One file's details: a size of `size` bytes.
const fn sized(size: u64) -> Details {
    Details {
        size: Some(size),
        modified: None,
    }
}

/// A window on [`SHOWN`] whose listing has arrived with the details of
/// its first `with_details` entries only.
fn shown_with_details(
    cx: &mut TestAppContext,
    with_details: u32,
) -> (Entity<MainWindow>, &mut VisualTestContext, Core, ListingId) {
    let (view, cx, mut core) = window(cx);
    let id = answer_listing(&mut core, ENTRIES);
    core.send(Event::DetailsRead {
        id,
        first: EntryId(0),
        details: (0..with_details).map(|_| sized(1)).collect(),
    });
    cx.run_until_parked();
    (view, cx, core, id)
}

#[gpui::test]
fn a_refused_folder_reads_the_shown_one_again_while_its_details_are_missing(
    cx: &mut TestAppContext,
) {
    let (view, cx, mut core, _) = shown_with_details(cx, 1);

    view.update(cx, |view, _| view.open(Path::new(MISSING)));
    let (id, path) = core.next_listing().unwrap();
    core.refuse(id, &path);
    cx.run_until_parked();

    let (_, again) = core.next_listing().unwrap();
    assert_eq!(again, Path::new(SHOWN));
}

#[gpui::test]
fn a_refused_folder_leaves_a_complete_listing_alone(cx: &mut TestAppContext) {
    let (view, cx, mut core, _) = shown_with_details(cx, ENTRIES);

    view.update(cx, |view, _| view.open(Path::new(MISSING)));
    let (id, path) = core.next_listing().unwrap();
    core.refuse(id, &path);
    cx.run_until_parked();

    assert!(core.next_listing().is_none());
}

#[gpui::test]
fn the_shown_folder_refused_too_is_not_asked_for_again(cx: &mut TestAppContext) {
    let (view, cx, mut core, _) = shown_with_details(cx, 1);

    view.update(cx, |view, _| view.open(Path::new(MISSING)));
    let (id, path) = core.next_listing().unwrap();
    core.refuse(id, &path);
    cx.run_until_parked();
    let (id, path) = core.next_listing().unwrap();
    core.refuse(id, &path);
    cx.run_until_parked();

    assert!(core.next_listing().is_none());
    view.read_with(cx, |view, _| {
        assert_eq!(view.folder(), Some(Path::new(SHOWN)));
        assert!(!view.waiting());
    });
}

#[gpui::test]
fn details_for_another_listing_are_left_out(cx: &mut TestAppContext) {
    let (view, cx, core, id) = shown_with_details(cx, 1);

    core.send(Event::DetailsRead {
        id: ListingId(id.0 + 1),
        first: EntryId(1),
        details: Arc::new([sized(5)]),
    });
    cx.run_until_parked();

    view.read_with(cx, |view, _| assert_eq!(view.columns(1).unwrap().size, ""));
}

#[gpui::test]
fn details_after_a_gap_are_left_out(cx: &mut TestAppContext) {
    let (view, cx, core, id) = shown_with_details(cx, 1);

    core.send(Event::DetailsRead {
        id,
        first: EntryId(2),
        details: Arc::new([sized(5)]),
    });
    cx.run_until_parked();

    view.read_with(cx, |view, _| assert_eq!(view.columns(2).unwrap().size, ""));
}

const SCROLLED: Point<gpui::Pixels> = point(px(0.0), px(-240.0));

/// A window on [`SHOWN`] with many rows, scrolled down to [`SCROLLED`].
fn scrolled(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, MANY);
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        view.scroll().0.borrow().base_handle.set_offset(SCROLLED);
    });
    (view, cx, core)
}

fn offset(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Point<gpui::Pixels> {
    view.read_with(cx, |view, _| view.scroll().0.borrow().base_handle.offset())
}

#[gpui::test]
fn reading_the_shown_folder_again_keeps_its_scroll_position(cx: &mut TestAppContext) {
    let (view, cx, mut core) = scrolled(cx);

    view.update(cx, |view, _| view.open(Path::new(SHOWN)));
    answer_listing(&mut core, MANY);
    cx.run_until_parked();

    assert_eq!(offset(&view, cx), SCROLLED);
}

#[gpui::test]
fn another_folder_starts_at_the_top(cx: &mut TestAppContext) {
    let (view, cx, mut core) = scrolled(cx);

    view.update(cx, |view, _| view.open(Path::new(OTHER)));
    answer_listing(&mut core, MANY);
    cx.run_until_parked();

    assert_eq!(offset(&view, cx), Point::default());
}
