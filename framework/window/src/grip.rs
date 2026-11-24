use gpui::{
    App, ClickEvent, Context, CursorStyle, Div, InteractiveElement as _, MouseButton,
    MouseDownEvent, MouseMoveEvent, ParentElement as _, Pixels, Stateful,
    StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _, px,
};
use nova_files_shell::{Published, Resizable, Widths};

use crate::theme::{Colors, size, space};
use crate::window::MainWindow;

const DOUBLE_CLICK: usize = 2;

#[derive(Debug, Clone, Copy)]
pub struct Held {
    part: Resizable,
    from: Pixels,
    start: Pixels,
    width: Pixels,
}

const fn bounds(part: Resizable) -> (Pixels, Pixels) {
    match part {
        Resizable::Sidebar => (size::SIDEBAR_NARROWEST, size::SIDEBAR_WIDEST),
        Resizable::Folder | Resizable::Kind | Resizable::Modified | Resizable::Size => {
            (size::COLUMN_NARROWEST, size::COLUMN_WIDEST)
        }
    }
}

const fn grows_rightward(part: Resizable) -> bool {
    matches!(part, Resizable::Sidebar)
}

pub fn kept_width(part: Resizable, cx: &App) -> Option<Pixels> {
    Published::<Widths>::get(cx)
        .and_then(|widths| widths.of(part))
        .map(|width| px(f32::from(width)))
}

impl MainWindow {
    pub(crate) fn width_of(&self, part: Resizable, cx: &App) -> Pixels {
        match self.grip {
            Some(held) if held.part == part => held.width,
            _ => kept_width(part, cx).unwrap_or_else(|| self.default_width(part)),
        }
    }

    fn default_width(&self, part: Resizable) -> Pixels {
        match part {
            Resizable::Size if self.shows_shares() => space::COLUMN_WIDTH,
            Resizable::Sidebar => size::SIDEBAR_WIDTH,
            Resizable::Folder => size::FOLDER_WIDTH,
            Resizable::Kind => size::KIND_WIDTH,
            Resizable::Modified => size::MODIFIED_WIDTH,
            Resizable::Size => size::SIZE_WIDTH,
        }
    }

    fn grip_pressed(&mut self, part: Resizable, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let start = self.width_of(part, cx);
        self.grip = Some(Held {
            part,
            from: event.position.x,
            start,
            width: start,
        });
        cx.notify();
    }

    pub(crate) fn grip_moved(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(held) = &mut self.grip else { return };
        let moved = event.position.x - held.from;
        let grown = if grows_rightward(held.part) {
            moved
        } else {
            -moved
        };
        let (narrowest, widest) = bounds(held.part);
        held.width = (held.start + grown).clamp(narrowest, widest).round();
        cx.notify();
    }

    pub(crate) fn grip_released(&mut self, cx: &mut Context<Self>) {
        let Some(held) = self.grip.take() else { return };
        if held.width != held.start {
            keep_width(held.part, Some(held.width), cx);
        }
        cx.notify();
    }

    pub(crate) fn grip(&self, part: Resizable, cx: &Context<Self>) -> Stateful<Div> {
        let colors = Colors::of(cx);
        let held = self.grip.is_some_and(|held| held.part == part);
        let selector = format!("grip-{}", part.name());
        let line = div()
            .w(size::GRIP_LINE)
            .h_full()
            .rounded_full()
            .when(held, |line| line.bg(colors.text_faint));
        div()
            .id(selector.clone())
            .debug_selector(move || selector)
            .absolute()
            .top_0()
            .bottom_0()
            .w(size::GRIP_REACH)
            .flex()
            .justify_center()
            .cursor(CursorStyle::ResizeLeftRight)
            .group("grip")
            .child(line.group_hover("grip", |line| line.bg(colors.text_faint)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.grip_pressed(part, event, cx);
                }),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if event.click_count() == DOUBLE_CLICK {
                    this.grip = None;
                    keep_width(part, None, cx);
                    cx.notify();
                }
            }))
    }
}

fn keep_width(part: Resizable, width: Option<Pixels>, cx: &mut App) {
    let mut widths = Published::<Widths>::get(cx).cloned().unwrap_or_default();
    match width {
        Some(width) => {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a whole number of pixels kept within the part's bounds"
            )]
            let kept = f32::from(width).round() as u16;
            widths.0.insert(part.name().to_owned(), kept);
        }
        None => {
            widths.0.remove(part.name());
        }
    }
    Published::set(widths, cx);
}
