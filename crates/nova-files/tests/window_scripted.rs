//! The window against a scripted core: the test reads each command the
//! window sends and answers it by hand, so the order of events is exact.
//! Selecting is tested in `scripted_select`.

#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

mod scripted_select;

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use gpui::{Entity, Point, TestAppContext, VisualTestContext, point, px};
use nova_files_api::{
    Appearance, Command, CoreMessage, Details, Entry, EntryId, EntryKind, Event, EventSink,
    Listing, ListingId, Mode, Palette, SettingChange, Settings, Theme, Thumbnails, Unreadable,
};
use nova_files_ui::{Colors, MainWindow};

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

    /// The thumbnail requests the window has sent since last asked, as
    /// `(first, count)`, skipping other commands.
    fn thumbnail_requests(&self) -> Vec<(u32, u32)> {
        let mut requests = Vec::new();
        while let Ok(message) = self.messages.try_recv() {
            if let CoreMessage::Command {
                command: Command::Rows { first, count, .. },
                ..
            } = message
            {
                requests.push((first.0, count));
            }
        }
        requests
    }

    /// Every command the window has sent since last asked.
    fn commands(&self) -> Vec<Command> {
        let mut commands = Vec::new();
        while let Ok(message) = self.messages.try_recv() {
            if let CoreMessage::Command { command, .. } = message {
                commands.push(command);
            }
        }
        commands
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
fn a_refused_folder_leaves_the_shown_one_alone(cx: &mut TestAppContext) {
    let (view, cx, mut core, _) = shown_with_details(cx, 1);

    view.update(cx, |view, _| view.open(Path::new(MISSING)));
    view.read_with(cx, |view, _| assert!(view.waiting()));
    let (id, path) = core.next_listing().unwrap();
    core.refuse(id, &path);
    cx.run_until_parked();

    assert!(core.next_listing().is_none());
    view.read_with(cx, |view, _| {
        assert_eq!(view.folder(), Some(Path::new(SHOWN)));
        assert!(!view.waiting());
        assert_eq!(view.columns(0).unwrap().size, "1 B");
    });
}

/// Opens [`OTHER`] then [`MISSING`] on a window showing [`SHOWN`], answers
/// [`OTHER`] (too late: the window wants [`MISSING`] by then) and refuses
/// [`MISSING`].
fn drop_a_listing_then_refuse(
    view: &Entity<MainWindow>,
    cx: &mut VisualTestContext,
    core: &mut Core,
) {
    view.update(cx, |view, _| {
        view.open(Path::new(OTHER));
        view.open(Path::new(MISSING));
    });
    answer_listing(core, ENTRIES);
    let (id, path) = core.next_listing().unwrap();
    core.refuse(id, &path);
    cx.run_until_parked();
}

#[gpui::test]
fn a_refused_folder_reads_the_shown_one_again_after_a_dropped_listing(cx: &mut TestAppContext) {
    let (view, cx, mut core, _) = shown_with_details(cx, 1);

    drop_a_listing_then_refuse(&view, cx, &mut core);

    let (_, again) = core.next_listing().unwrap();
    assert_eq!(again, Path::new(SHOWN));
    assert!(core.next_listing().is_none());
}

#[gpui::test]
fn a_listing_dropped_after_the_refusal_reads_the_shown_one_again(cx: &mut TestAppContext) {
    let (view, cx, mut core, _) = shown_with_details(cx, 1);
    view.update(cx, |view, _| {
        view.open(Path::new(OTHER));
        view.open(Path::new(MISSING));
    });
    let (other, other_path) = core.next_listing().unwrap();
    let (missing, missing_path) = core.next_listing().unwrap();

    core.refuse(missing, &missing_path);
    cx.run_until_parked();
    assert!(core.next_listing().is_none());
    core.send(Event::FolderListed(Listing {
        id: other,
        path: other_path,
        entries: Arc::new([]),
    }));
    cx.run_until_parked();

    let (_, again) = core.next_listing().unwrap();
    assert_eq!(again, Path::new(SHOWN));
}

#[gpui::test]
fn an_older_listing_dropped_leaves_the_shown_one_alone(cx: &mut TestAppContext) {
    let (_view, cx, mut core, id) = shown_with_details(cx, 1);

    core.send(Event::FolderListed(Listing {
        id: ListingId(id.0.saturating_sub(1)),
        path: PathBuf::from(OTHER),
        entries: Arc::new([]),
    }));
    cx.run_until_parked();

    assert!(core.next_listing().is_none());
}

#[gpui::test]
fn the_shown_folder_refused_too_is_not_asked_for_again(cx: &mut TestAppContext) {
    let (view, cx, mut core, _) = shown_with_details(cx, 1);
    drop_a_listing_then_refuse(&view, cx, &mut core);

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
fn details_after_a_gap_fill_their_own_rows(cx: &mut TestAppContext) {
    let (view, cx, core, id) = shown_with_details(cx, 1);

    core.send(Event::DetailsRead {
        id,
        first: EntryId(2),
        details: Arc::new([sized(5)]),
    });
    cx.run_until_parked();

    view.read_with(cx, |view, _| {
        assert_eq!(view.columns(1).unwrap().size, "");
        assert_eq!(view.columns(2).unwrap().size, "5 B");
    });
}

#[gpui::test]
fn skipping_a_place_gone_redraws_the_window(cx: &mut TestAppContext) {
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    view.update(cx, |view, _| view.open(Path::new(OTHER)));
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    view.update(cx, |view, _| view.go_back());
    let (id, path) = core.next_listing().unwrap();
    let redraws = Rc::new(Cell::new(0));
    let counted = Rc::clone(&redraws);
    let _watch = cx.update(|_, cx| cx.observe(&view, move |_, _| counted.set(counted.get() + 1)));

    core.refuse(id, &path);
    cx.run_until_parked();

    // The Back button has to grey out: nothing is left behind.
    view.read_with(cx, |view, _| assert_eq!(view.back_target(), None));
    assert!(redraws.get() > 0);
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

/// A 4 by 4 red PNG.
const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4, 0, 0, 0, 4, 8, 6, 0,
    0, 0, 169, 241, 158, 126, 0, 0, 0, 18, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31,
    25, 51, 144, 46, 0, 0, 60, 64, 31, 225, 224, 129, 119, 180, 0, 0, 0, 0, 73, 69, 78, 68, 174,
    66, 96, 130,
];

fn thumbnail(id: ListingId, entry: u32) -> Event {
    Event::Thumbnail {
        id,
        entry: EntryId(entry),
        png: PNG.into(),
    }
}

#[gpui::test]
fn a_thumbnail_for_a_shown_row_is_kept(cx: &mut TestAppContext) {
    let (view, cx, core, id) = shown_with_details(cx, ENTRIES);

    core.send(thumbnail(id, 1));
    cx.run_until_parked();

    view.read_with(cx, |view, _| {
        assert!(view.thumbnail(1).is_some());
        assert!(view.thumbnail(0).is_none());
    });
}

#[gpui::test]
fn a_thumbnail_for_a_listing_left_behind_is_dropped(cx: &mut TestAppContext) {
    let (view, cx, mut core, id) = shown_with_details(cx, ENTRIES);

    // Read again: the thumbnail for the old listing lands while the new
    // one is already on screen.
    view.update(cx, |view, _| view.open(Path::new(SHOWN)));
    core.send(thumbnail(id, 1));
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();

    view.read_with(cx, |view, _| assert!(view.thumbnail(1).is_none()));
}

#[gpui::test]
fn the_same_scale_keeps_the_thumbnails(cx: &mut TestAppContext) {
    let (view, cx, core, id) = shown_with_details(cx, ENTRIES);
    core.send(thumbnail(id, 1));
    cx.run_until_parked();
    core.thumbnail_requests();

    let scale = cx.update(|window, _| window.scale_factor());
    view.update_in(cx, |view, window, cx| {
        view.set_scale_factor(scale, window, cx);
    });

    view.read_with(cx, |view, _| assert!(view.thumbnail(1).is_some()));
    assert_eq!(core.thumbnail_requests(), []);
}

#[gpui::test]
fn another_scale_drops_the_thumbnails_and_asks_again(cx: &mut TestAppContext) {
    let (view, cx, core, id) = shown_with_details(cx, ENTRIES);
    core.send(thumbnail(id, 1));
    cx.run_until_parked();
    core.thumbnail_requests();

    let scale = cx.update(|window, _| window.scale_factor());
    view.update_in(cx, |view, window, cx| {
        view.set_scale_factor(scale * 2.0, window, cx);
    });

    view.read_with(cx, |view, _| assert!(view.thumbnail(1).is_none()));
    assert_eq!(core.thumbnail_requests(), [(0, 0)]);
}

#[gpui::test]
fn a_thumbnail_arriving_asks_for_nothing_new(cx: &mut TestAppContext) {
    let (view, cx, mut core) = window(cx);
    let id = answer_listing(&mut core, MANY);
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        view.scroll().0.borrow().base_handle.set_offset(SCROLLED);
        cx.notify();
    });
    cx.run_until_parked();
    let (first, count) = *core.thumbnail_requests().last().unwrap();
    let row = first + count / 2;

    core.send(thumbnail(id, row));
    cx.run_until_parked();

    view.read_with(cx, |view, _| {
        assert!(view.thumbnail(usize::try_from(row).unwrap()).is_some());
    });
    assert_eq!(core.thumbnail_requests(), []);
}

#[gpui::test]
fn the_window_asks_for_the_settings_and_the_theme(cx: &mut TestAppContext) {
    let (_view, _cx, core) = window(cx);
    let commands = core.commands();
    assert!(commands.contains(&Command::Settings), "{commands:?}");
    assert!(commands.contains(&Command::Theme), "{commands:?}");
}

#[gpui::test]
fn the_window_draws_with_the_theme_the_core_sends(cx: &mut TestAppContext) {
    let (_view, cx, mut core) = window(cx);
    core.next_listing();
    assert_eq!(cx.update(|_, cx| Colors::of(cx)), Colors::default());
    let light = Theme {
        mode: Mode::Light,
        palette: Palette::carbonfox(Mode::Light),
    };
    core.send(Event::Theme(Arc::new(light.clone())));
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| Colors::of(cx)), Colors::new(&light));
}

#[gpui::test]
fn hidden_thumbnails_are_let_go_and_shown_ones_asked_for_again(cx: &mut TestAppContext) {
    let (view, cx, core, id) = shown_with_details(cx, ENTRIES);
    core.send(thumbnail(id, 1));
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert!(view.thumbnail(1).is_some()));

    let hidden = Settings::default().with(SettingChange::Thumbnails(Thumbnails::Hidden));
    core.send(Event::Settings(Arc::new(hidden.clone())));
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert!(view.thumbnail(1).is_none()));
    // One already on its way is ignored.
    core.send(thumbnail(id, 2));
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert!(view.thumbnail(2).is_none()));

    // Settings that leave thumbnails alone don't ask for rows again.
    core.commands();
    core.send(Event::Settings(Arc::new(
        hidden.with(SettingChange::Appearance(Appearance::Dark)),
    )));
    cx.run_until_parked();
    let asks_rows = |commands: Vec<Command>| {
        commands
            .iter()
            .any(|command| matches!(command, Command::Rows { .. }))
    };
    assert!(!asks_rows(core.commands()));

    core.send(Event::Settings(Arc::default()));
    cx.run_until_parked();
    assert!(asks_rows(core.commands()));
    core.send(thumbnail(id, 2));
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert!(view.thumbnail(2).is_some()));
}

#[gpui::test]
fn hiding_and_showing_before_a_frame_still_asks_for_thumbnails_anew(cx: &mut TestAppContext) {
    let (view, cx, core, id) = shown_with_details(cx, ENTRIES);
    core.send(thumbnail(id, 1));
    cx.run_until_parked();
    core.thumbnail_requests();
    let hidden = Settings::default().with(SettingChange::Thumbnails(Thumbnails::Hidden));
    core.send(Event::Settings(Arc::new(hidden)));
    core.send(Event::Settings(Arc::default()));
    cx.run_until_parked();
    // The core is told to forget what it sent, then asked again.
    let requests = core.thumbnail_requests();
    let forgot = requests.iter().position(|&request| request == (0, 0));
    let asked = requests.iter().rposition(|&(_, count)| count > 0);
    assert!(forgot < asked && forgot.is_some(), "{requests:?}");
    view.read_with(cx, |view, _| assert!(view.thumbnail(1).is_none()));
}

#[gpui::test]
fn a_thumbnail_shrunk_after_hiding_is_not_kept(cx: &mut TestAppContext) {
    let (view, cx, core, id) = shown_with_details(cx, ENTRIES);
    let hidden = Settings::default().with(SettingChange::Thumbnails(Thumbnails::Hidden));
    // On its way when thumbnails are hidden.
    core.send(thumbnail(id, 1));
    core.send(Event::Settings(Arc::new(hidden)));
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert!(view.thumbnail(1).is_none()));
}
