use std::cell::{Cell, RefCell};
use std::path::Path;

use gpui::{Context, Window};
use nova_files_api::{Entry, EntryId};
use nova_files_shell::actions::CursorDown;
use nova_files_shell::{Host, Move, Part, Parts, Showing, Slots};

use super::super::*;

thread_local! {
    static TAKING: Cell<bool> = const { Cell::new(false) };
    static TAKEN: Cell<usize> = const { Cell::new(0) };
    static OPENED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

struct Peeker;

impl Part for Peeker {
    fn open(_: &Host, _: &mut Context<Self>) -> Self {
        Self
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.before(|_, _: &CursorDown, _: &mut Window, _| {
            let taking = TAKING.get();
            if taking {
                TAKEN.set(TAKEN.get() + 1);
            }
            taking
        });
        slots.opens(|_, entry: &Entry, _: &Path, _| {
            OPENED.with_borrow_mut(|opened| opened.push(entry.name.to_string_lossy().into()));
            true
        });
    }
}

fn peeking(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    TAKING.set(false);
    TAKEN.set(0);
    let mut parts = Parts::default();
    parts.add::<Peeker>();
    cx.update(|cx| {
        cx.set_global(parts);
        nova_files_ui::bind_window_keys(cx);
    });
    let (view, cx, mut core) = window_with_parts(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    (view, cx, core)
}

fn shown_cursor(view: &Entity<MainWindow>, cx: &VisualTestContext) -> Option<usize> {
    view.read_with(cx, |view, cx| view.host().entries.read(cx).cursor())
}

#[gpui::test]
fn a_part_takes_a_key_first_only_when_it_says_so(cx: &mut TestAppContext) {
    let (view, cx, _core) = peeking(cx);
    cx.simulate_keystrokes("home down");
    cx.run_until_parked();
    assert_eq!(shown_cursor(&view, cx), Some(1));
    assert_eq!(TAKEN.get(), 0);

    TAKING.set(true);
    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    assert_eq!(shown_cursor(&view, cx), Some(1));
    assert_eq!(TAKEN.get(), 1);
}

#[gpui::test]
fn a_part_moves_the_cursor_and_opens_an_entry(cx: &mut TestAppContext) {
    let (view, cx, _core) = peeking(cx);
    let entries = view.read_with(cx, |view, _| view.host().entries.clone());
    entries.update(cx, |entries, cx| {
        entries.move_cursor(Some(0), Move::Last, cx);
    });
    cx.run_until_parked();
    assert_eq!(shown_cursor(&view, cx), Some(2));

    entries.update(cx, |entries, cx| entries.open(EntryId(1), cx));
    cx.run_until_parked();
    assert_eq!(OPENED.with_borrow(Clone::clone), ["1"]);
    let showing = view.read_with(cx, |view, cx| view.host().scene(cx).showing);
    assert_eq!(showing, Showing::List);
}

#[gpui::test]
fn a_part_sees_a_type_jump_running_and_the_list_focused(cx: &mut TestAppContext) {
    let (view, cx, _core) = peeking(cx);
    cx.simulate_keystrokes("home");
    cx.run_until_parked();
    let (list, entries) = view.read_with(cx, |view, _| {
        (view.host().list.clone(), view.host().entries.clone())
    });
    assert!(cx.update(|window, _| list.is_focused(window)));
    let now = cx.update(|_, cx| cx.background_executor().now());
    assert!(!entries.read_with(cx, |entries, _| entries.jumping(now)));

    cx.simulate_keystrokes("1");
    cx.run_until_parked();
    let now = cx.update(|_, cx| cx.background_executor().now());
    assert!(entries.read_with(cx, |entries, _| entries.jumping(now)));
}
