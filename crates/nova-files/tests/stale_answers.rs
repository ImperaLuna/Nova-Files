//! A window against a stand-in core that answers by hand, in an order the
//! real core only produces in a race: answers for a folder the window has
//! since moved away from must not move it there.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::Receiver;

use gpui::TestAppContext;
use nova_files_api::{Command, CoreMessage, Event, EventSink, Listing, ListingId, Unreadable};
use nova_files_ui::{MainWindow, WindowEvent};

/// The next `ListFolder` the window sent, skipping other messages.
fn next_listing(core: &Receiver<CoreMessage>) -> (ListingId, PathBuf) {
    loop {
        if let CoreMessage::Command {
            command: Command::ListFolder { id, path },
            ..
        } = core.try_recv().unwrap()
        {
            return (id, path);
        }
    }
}

/// The window's event sink, from its `Connected` message.
fn window_sink(core: &Receiver<CoreMessage>) -> Box<dyn EventSink> {
    match core.try_recv().unwrap() {
        CoreMessage::Connected { events, .. } => events,
        other => panic!("expected the window to connect first, got {other:?}"),
    }
}

fn listed(id: ListingId, path: PathBuf) -> Event {
    Event::FolderListed(Listing {
        id,
        path,
        entries: Vec::new().into(),
    })
}

#[gpui::test]
fn answers_for_a_folder_left_behind_are_dropped(cx: &mut TestAppContext) {
    let (connector, core) = nova_files_api::channel();
    let (view, cx) =
        cx.add_window_view(|_, cx| MainWindow::new(&connector, Path::new("/first"), cx));
    let opened = Rc::new(RefCell::new(Vec::new()));
    let _watch = cx.update(|_, cx| {
        let opened = Rc::clone(&opened);
        cx.subscribe(&view, move |_, event: &WindowEvent, _| {
            if let WindowEvent::Opened(path) | WindowEvent::Refused(path) = event {
                opened.borrow_mut().push(path.clone());
            }
        })
    });
    let sink = window_sink(&core);
    let (first, first_path) = next_listing(&core);
    sink.deliver(listed(first, first_path)).unwrap();
    cx.run_until_parked();

    view.update(cx, |view, _| {
        view.open(Path::new("/second"));
        view.open(Path::new("/third"));
    });
    let (second, second_path) = next_listing(&core);
    let (third, third_path) = next_listing(&core);
    sink.deliver(listed(second, second_path.clone())).unwrap();
    sink.deliver(Event::FolderUnreadable {
        id: second,
        path: second_path,
        reason: Unreadable::Missing,
    })
    .unwrap();
    sink.deliver(listed(third, third_path)).unwrap();
    cx.run_until_parked();

    assert_eq!(
        *opened.borrow(),
        [PathBuf::from("/first"), PathBuf::from("/third")]
    );
}
