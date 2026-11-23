use std::time::Duration;

use gpui::{
    Context, Div, InteractiveElement as _, MouseButton, MouseDownEvent, MouseMoveEvent,
    ParentElement as _, Pixels, Rgba, Stateful, Styled as _, Task, div, point,
    prelude::FluentBuilder as _, px,
};

use crate::theme::{Colors, size};
use crate::window::MainWindow;

const AWAKE: Duration = Duration::from_secs(1);
const REACH: Pixels = px(16.0);
const THIN: Pixels = px(4.0);
const WIDE: Pixels = px(8.0);
const INSET: Pixels = px(3.0);
const TOP: Pixels = px(8.0);
const SHORTEST: Pixels = px(32.0);
const RESTING: f32 = 0.32;
const POINTED: f32 = 0.52;
const HELD: f32 = 0.66;
const GRAB_MIDDLE: f32 = 0.5;

#[derive(Default)]
pub struct ScrollBar {
    seen: Option<Pixels>,
    shown: bool,
    hiding: Option<Task<()>>,
    held: Option<Held>,
}

#[derive(Clone, Copy)]
struct Held {
    from: Pixels,
    scrolled: Pixels,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thumb {
    pub top: Pixels,
    pub height: Pixels,
}

pub fn thumb(track: Pixels, view: Pixels, most: Pixels, scrolled: Pixels) -> Option<Thumb> {
    if most <= px(0.0) || track <= SHORTEST {
        return None;
    }
    let height = (track * (view / (view + most))).max(SHORTEST).min(track);
    let top = (track - height) * (scrolled / most).clamp(0.0, 1.0);
    Some(Thumb { top, height })
}

impl MainWindow {
    pub const fn scroll_bar_shown(&self) -> bool {
        self.bar.shown || self.bar.held.is_some()
    }

    fn bar_track(&self) -> (Pixels, Pixels, Pixels, Pixels) {
        let handle = self.scroll.0.borrow().base_handle.clone();
        let view = handle.bounds().size.height;
        let track = view - TOP - size::ISLAND_CLEARANCE;
        (track, view, handle.max_offset().y, -handle.offset().y)
    }

    fn bar_thumb(&self) -> Option<Thumb> {
        let (track, view, most, scrolled) = self.bar_track();
        thumb(track, view, most, scrolled)
    }

    pub(crate) fn bar_watch(&mut self, cx: &Context<Self>) {
        let scrolled = self.bar_track().3;
        let moved = self.bar.seen.is_some_and(|seen| seen != scrolled);
        self.bar.seen = Some(scrolled);
        if moved {
            self.bar_wake(cx);
        }
    }

    fn bar_wake(&mut self, cx: &Context<Self>) {
        self.bar.shown = true;
        self.bar.hiding = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AWAKE).await;
            this.update(cx, |this, cx| {
                this.bar.shown = false;
                cx.notify();
            })
            .ok();
        }));
    }

    fn bar_scroll_to(&mut self, scrolled: Pixels, cx: &mut Context<Self>) {
        let handle = self.scroll.0.borrow().base_handle.clone();
        let most = handle.max_offset().y;
        let offset = handle.offset();
        handle.set_offset(point(offset.x, -scrolled.clamp(px(0.0), most)));
        self.bar_wake(cx);
        self.band_scrolled(cx);
        cx.notify();
    }

    fn bar_pressed(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some(thumb) = self.bar_thumb() else {
            return;
        };
        let (track, view, most, scrolled) = self.bar_track();
        let at = event.position.y - self.list_bounds().top() - TOP;
        if event.modifiers.shift {
            let share = (at - thumb.height * GRAB_MIDDLE) / (track - thumb.height).max(px(1.0));
            self.bar_scroll_to(most * share, cx);
        } else if at < thumb.top {
            self.bar_scroll_to(scrolled - view, cx);
            return;
        } else if at > thumb.top + thumb.height {
            self.bar_scroll_to(scrolled + view, cx);
            return;
        }
        self.bar.held = Some(Held {
            from: event.position.y,
            scrolled: self.bar_track().3,
        });
        cx.notify();
    }

    pub(crate) fn bar_moved(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(held) = self.bar.held else { return };
        if event.pressed_button != Some(MouseButton::Left) {
            self.bar_released(cx);
            return;
        }
        let Some(thumb) = self.bar_thumb() else {
            return;
        };
        let (track, _, most, _) = self.bar_track();
        let room = (track - thumb.height).max(px(1.0));
        self.bar_scroll_to(
            held.scrolled + most * ((event.position.y - held.from) / room),
            cx,
        );
    }

    pub(crate) fn bar_released(&mut self, cx: &mut Context<Self>) {
        if self.bar.held.take().is_some() {
            self.bar_wake(cx);
            cx.notify();
        }
    }

    pub(crate) fn scroll_bar(&self, cx: &Context<Self>) -> Option<Stateful<Div>> {
        let thumb = self.bar_thumb()?;
        let colors = Colors::of(cx);
        let held = self.bar.held.is_some();
        let shown = self.scroll_bar_shown();
        Some(
            div()
                .id("scroll-bar")
                .debug_selector(|| "scroll-bar".to_owned())
                .group("scroll-bar")
                .absolute()
                .top(TOP)
                .bottom(size::ISLAND_CLEARANCE)
                .right_0()
                .w(REACH)
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.bar_pressed(event, cx);
                    }),
                )
                .child(
                    div()
                        .debug_selector(|| "scroll-thumb".to_owned())
                        .absolute()
                        .top(thumb.top)
                        .h(thumb.height)
                        .right(INSET)
                        .w(if held { WIDE } else { THIN })
                        .rounded_full()
                        .bg(tint(colors.text, if held { HELD } else { RESTING }))
                        .when(!shown, |thumb| thumb.opacity(0.0))
                        .group_hover("scroll-bar", |thumb| {
                            thumb.w(WIDE).opacity(1.0).bg(tint(colors.text, POINTED))
                        }),
                ),
        )
    }
}

const fn tint(color: Rgba, alpha: f32) -> Rgba {
    Rgba { a: alpha, ..color }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thumb_shows_the_share_on_screen_and_where_it_is() {
        assert_eq!(thumb(px(400.0), px(500.0), px(0.0), px(0.0)), None);
        let top = thumb(px(400.0), px(500.0), px(1500.0), px(0.0)).unwrap();
        assert_eq!(
            top,
            Thumb {
                top: px(0.0),
                height: px(100.0)
            }
        );
        let end = thumb(px(400.0), px(500.0), px(1500.0), px(1500.0)).unwrap();
        assert_eq!(end.top, px(300.0));
        let huge = thumb(px(400.0), px(500.0), px(5_000_000.0), px(2_500_000.0)).unwrap();
        assert_eq!(huge.height, SHORTEST);
        assert_eq!(huge.top, (px(400.0) - SHORTEST) * 0.5);
    }
}
