//! The window against a scripted core: the test reads each command the
//! window sends and answers it by hand, so the order of events is exact.

#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use gpui::{Entity, TestAppContext, VisualTestContext};
use nova_files_api::{
    Command, CoreMessage, Details, Entry, EntryId, EntryKind, Event, EventSink, Listing, ListingId,
    Unreadable,
};
use nova_files_ui::MainWindow;

const SHOWN: &str = "/shown";
const MISSING: &str = "/shown/missing";
const ENTRIES: u32 = 3;

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

/// A window on [`SHOWN`] whose listing has arrived with the details of
/// its first `with_details` entries only.
fn shown_with_details(
    cx: &mut TestAppContext,
    with_details: u32,
) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    let (connector, messages) = nova_files_api::channel();
    let (view, cx) = cx.add_window_view(|_, cx| {
        MainWindow::with_home(&connector, Path::new(SHOWN), PathBuf::from(SHOWN), cx)
    });
    let mut core = Core {
        messages,
        events: None,
    };
    let (id, path) = core.next_listing().unwrap();
    let entries: Arc<[Entry]> = (0..ENTRIES)
        .map(|index| Entry {
            id: EntryId(index),
            name: format!("{index}").into(),
            kind: EntryKind::File,
        })
        .collect();
    core.send(Event::FolderListed(Listing { id, path, entries }));
    core.send(Event::DetailsRead {
        id,
        first: EntryId(0),
        details: (0..with_details).map(|_| Details::default()).collect(),
    });
    cx.run_until_parked();
    (view, cx, core)
}

#[gpui::test]
fn a_refused_folder_reads_the_shown_one_again_while_its_details_are_missing(
    cx: &mut TestAppContext,
) {
    let (view, cx, mut core) = shown_with_details(cx, 1);

    view.update(cx, |view, _| view.open(Path::new(MISSING)));
    let (id, path) = core.next_listing().unwrap();
    core.refuse(id, &path);
    cx.run_until_parked();

    let (_, again) = core.next_listing().unwrap();
    assert_eq!(again, Path::new(SHOWN));
}

#[gpui::test]
fn a_refused_folder_leaves_a_complete_listing_alone(cx: &mut TestAppContext) {
    let (view, cx, mut core) = shown_with_details(cx, ENTRIES);

    view.update(cx, |view, _| view.open(Path::new(MISSING)));
    let (id, path) = core.next_listing().unwrap();
    core.refuse(id, &path);
    cx.run_until_parked();

    assert!(core.next_listing().is_none());
}

#[gpui::test]
fn the_shown_folder_refused_too_is_not_asked_for_again(cx: &mut TestAppContext) {
    let (view, cx, mut core) = shown_with_details(cx, 1);

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
