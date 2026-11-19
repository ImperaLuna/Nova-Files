use std::cell::RefCell;
use std::ops::Range;

use gpui::Context;
use nova_files_api::ListingId;
use nova_files_shell::{Host, Part, Parts, Slots};

use super::super::*;

thread_local! {
    static DRAWN: RefCell<Vec<Range<usize>>> = const { RefCell::new(Vec::new()) };
    static SCALES: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
}

struct Watcher;

impl Part for Watcher {
    fn open(_: &Host, _: &mut Context<Self>) -> Self {
        Self
    }

    fn slots(slots: &mut Slots<Self>) {
        slots
            .rows_drawn(|_, _: ListingId, rows, _| DRAWN.with_borrow_mut(|drawn| drawn.push(rows)));
        slots.rescaled(|_, scale, _| SCALES.with_borrow_mut(|scales| scales.push(scale)));
    }
}

#[gpui::test]
fn a_part_hears_the_rows_drawn_and_the_scale_once(cx: &mut TestAppContext) {
    let mut parts = Parts::default();
    parts.add::<Watcher>();
    cx.update(|cx| cx.set_global(parts));
    let (view, cx, mut core) = window_with_parts(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();

    let scale = cx.update(|window, _| window.scale_factor());
    assert_eq!(SCALES.with_borrow(Clone::clone), [scale]);
    let rows = ENTRIES as usize;
    assert_eq!(
        DRAWN.with_borrow(|drawn| drawn.last().cloned()),
        Some(0..rows)
    );
    let read = view.read_with(cx, |view, cx| view.host().entries.read(cx).scale_factor());
    assert_eq!(read.to_bits(), scale.to_bits());
}
