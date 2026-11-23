use gpui::{Context, MouseButton, MouseMoveEvent, Pixels, Point, px};

use crate::window::MainWindow;

const LIFT_AFTER: Pixels = px(6.0);

#[derive(Debug, Clone, Copy)]
pub struct Lift {
    from: Point<Pixels>,
    lifted: bool,
}

impl MainWindow {
    pub(crate) fn entries_pressed(&mut self, at: Point<Pixels>, on_entry: bool) {
        self.lift = on_entry.then_some(Lift {
            from: at,
            lifted: false,
        });
    }

    pub(crate) fn lift_moved(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.lift_dropped(cx);
            return;
        }
        let Some(lift) = &mut self.lift else { return };
        let moved = (event.position.x - lift.from.x)
            .abs()
            .max((event.position.y - lift.from.y).abs());
        if !lift.lifted && moved >= LIFT_AFTER {
            lift.lifted = true;
            cx.notify();
        }
    }

    pub(crate) fn lift_dropped(&mut self, cx: &mut Context<Self>) {
        if self.lift.take().is_some_and(|lift| lift.lifted) {
            cx.notify();
        }
    }

    pub(crate) fn lifted(&self) -> bool {
        self.lift.is_some_and(|lift| lift.lifted)
    }
}
