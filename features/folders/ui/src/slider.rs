use gpui::{
    Anchor, AnyElement, Context, Div, InteractiveElement as _, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, ParentElement as _, Pixels, Point, Styled as _, anchored,
    deferred, div, px,
};
use nova_files_folders_api::{FolderView, IconSize};
use nova_files_shell::assets::Icon;
use nova_files_shell::theme::{Colors, radius, size};
use nova_files_shell::widgets::icon;

use crate::FoldersPart;

const TRACK_WIDTH: Pixels = px(132.0);
const STOP_HEIGHT: Pixels = px(24.0);
const LINE: Pixels = px(2.0);
const TICK: Pixels = px(6.0);
const KNOB: Pixels = px(16.0);
const SMALL_END: Pixels = px(13.0);
const BIG_END: Pixels = px(19.0);
const PAD_X: Pixels = px(12.0);
const PAD_Y: Pixels = px(8.0);
const DROP: Pixels = px(6.0);
const PRIORITY: usize = 1;
const HALF: f32 = 0.5;

#[derive(Default)]
pub struct SizeSlider {
    open: bool,
    closed_at: Option<Point<Pixels>>,
}

impl SizeSlider {
    pub const fn is_open(&self) -> bool {
        self.open
    }

    pub fn toggle(&mut self, pressed_at: Option<Point<Pixels>>) {
        let closed_by_this_press = pressed_at.is_some() && self.closed_at == pressed_at;
        self.closed_at = None;
        self.open = !self.open && !closed_by_this_press;
    }

    pub const fn close(&mut self) {
        self.open = false;
    }

    const fn closed_by(&mut self, pressed_at: Point<Pixels>) {
        self.open = false;
        self.closed_at = Some(pressed_at);
    }
}

pub fn size_slider(view: FolderView, cx: &Context<FoldersPart>) -> AnyElement {
    let colors = Colors::of(cx);
    let card = div()
        .id("icon-sizes")
        .debug_selector(|| "icon-sizes".to_owned())
        .flex()
        .items_center()
        .gap(size::GAP)
        .mt(DROP)
        .px(PAD_X)
        .py(PAD_Y)
        .rounded(radius::CONTROL)
        .border(size::LINE)
        .border_color(colors.line)
        .bg(colors.lifted)
        .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
            this.slider.closed_by(event.position);
            cx.notify();
        }))
        .child(icon(Icon::Tiles, SMALL_END, colors.text_dim))
        .child(track(view.icons, &colors, cx))
        .child(icon(Icon::Tiles, BIG_END, colors.text_dim));
    div()
        .absolute()
        .bottom_0()
        .right_0()
        .child(
            deferred(
                anchored()
                    .anchor(Anchor::TopRight)
                    .snap_to_window()
                    .child(card),
            )
            .with_priority(PRIORITY),
        )
        .into_any_element()
}

impl FoldersPart {
    fn slide_to(&mut self, icons: IconSize, cx: &mut Context<Self>) {
        let view = self.shown(cx);
        self.change_view(FolderView { icons, ..view }, cx);
    }
}

fn track(shown: IconSize, colors: &Colors, cx: &Context<FoldersPart>) -> Div {
    let stops = IconSize::ALL.len();
    #[expect(clippy::cast_precision_loss, reason = "six stops")]
    let stop_width = TRACK_WIDTH / stops as f32;
    #[expect(clippy::cast_precision_loss, reason = "six stops")]
    let reached = stop_width * shown.index() as f32;
    let rail = |width: Pixels, tint| {
        div()
            .absolute()
            .left(stop_width * HALF)
            .top((STOP_HEIGHT - LINE) * HALF)
            .w(width)
            .h(LINE)
            .rounded(radius::SHARE_BAR)
            .bg(tint)
    };
    div()
        .relative()
        .flex()
        .w(TRACK_WIDTH)
        .h(STOP_HEIGHT)
        .child(rail(TRACK_WIDTH - stop_width, colors.press))
        .child(rail(reached, colors.text_dim))
        .children(
            IconSize::ALL
                .into_iter()
                .map(|icons| stop(icons, shown, colors, cx)),
        )
}

fn stop(icons: IconSize, shown: IconSize, colors: &Colors, cx: &Context<FoldersPart>) -> Div {
    let mark = if icons == shown {
        div().size(KNOB).rounded_full().bg(colors.text)
    } else {
        let tint = if icons < shown {
            colors.text_dim
        } else {
            colors.text_faint
        };
        div().size(TICK).rounded_full().bg(tint)
    };
    div()
        .debug_selector(move || format!("icon-size-{}", icons.index()))
        .flex()
        .flex_1()
        .h_full()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| this.slide_to(icons, cx)),
        )
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            if event.pressed_button == Some(MouseButton::Left) {
                this.slide_to(icons, cx);
            }
        }))
        .child(mark)
}
