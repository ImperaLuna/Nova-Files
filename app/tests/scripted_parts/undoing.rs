use std::cell::Cell;
use std::rc::Rc;

use gpui::{App, Context};
use nova_files_shell::{Host, Island, OnUndo, Part, Parts, Slots, Undo};

use super::super::*;

thread_local! {
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

struct Undoer;

impl Part for Undoer {
    fn open(_: &Host, _: &mut Context<Self>) -> Self {
        Self
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.busy(|_, _: &App| BUSY.get());
    }
}

fn undoing(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    BUSY.set(false);
    let mut parts = Parts::default();
    parts.add::<Undoer>();
    cx.update(|cx| cx.set_global(parts));
    let (view, cx, mut core) = window_with_parts(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    (view, cx, core)
}

#[gpui::test]
fn a_part_takes_the_shown_note_undo_and_the_note_goes(cx: &mut TestAppContext) {
    let (view, cx, _core) = undoing(cx);
    let island = view.read_with(cx, |view, _| view.host().island.clone());
    let run: OnUndo = Rc::new(|_| {});
    island.update(cx, |island, cx| {
        island.note("Tagged", Some(Undo::Run(Rc::clone(&run))), cx);
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.note().is_some()));

    let taken = island.update(cx, Island::take_undo);
    cx.run_until_parked();

    assert_eq!(taken, Some(Undo::Run(run)));
    assert!(view.read_with(cx, |view, _| view.note().is_none()));
}

#[gpui::test]
fn no_undo_is_taken_while_a_part_is_busy(cx: &mut TestAppContext) {
    let (view, cx, _core) = undoing(cx);
    let island = view.read_with(cx, |view, _| view.host().island.clone());
    island.update(cx, |island, cx| {
        island.note("Tagged", Some(Undo::Run(Rc::new(|_| {}))), cx);
    });
    BUSY.set(true);
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();

    assert!(island.read_with(cx, |island, _| island.busy()));
    assert_eq!(island.update(cx, Island::take_undo), None);
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.note().is_some()));
}
