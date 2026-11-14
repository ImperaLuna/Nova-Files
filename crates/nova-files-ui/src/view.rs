//! Draws the main window as on the design page: the sidebar and the top
//! bar sit on the window itself, and the folder's entries are one sheet
//! that runs to the window's right and bottom edges, with a rounded corner
//! where the sidebar and the bar meet. In pick mode the pick bar runs along
//! the bottom (see `pick`).
//!
//! A narrow window folds the sidebar into one row above the bar (see
//! `sidebar`).
//!
//! Each part is drawn in a file of its own, so a feature changes only its
//! part: the top bar in `bar`, the sidebar in `sidebar`, the sheet with its
//! column heads and rows in `sheet`, the pieces of one row in `row`, a
//! right-click menu in `menu`, and the Start page, drawn on the sheet in
//! place of the rows, in `start`.
//!
//! The window sets the key context its actions are bound in and handles
//! each action it takes; buttons and the mouse's side buttons send actions
//! rather than doing the work (see `commands`). A press on the entries
//! that isn't on a name can drag a box to select (see `band`). Every
//! element a test clicks carries a debug selector: `back`, `forward`, `up`,
//! `place-<n>`, `entry-<n>` and its name `name-<n>`, counted from 0, and
//! the box `band`. Each line's Modified cell carries one too,
//! `modified-header` and `modified-<n>`, so a test can check the columns
//! line up. The slots later features fill carry one each: `sidebar`,
//! `path`, `search` and `views`.

mod bar;
mod menu;
mod row;
mod sheet;
mod sidebar;
mod start;

use gpui::{
    Context, Div, Focusable as _, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, NavigationDirection, ParentElement as _, Render,
    ScrollWheelEvent, Styled as _, Window, div, prelude::FluentBuilder as _,
};

use crate::commands::{
    self, ClearSelection, CursorDown, CursorFirst, CursorLast, CursorPageDown, CursorPageUp,
    CursorUp, GoBack, GoForward, GoUp, OpenSelected, SelectAll, SelectDown, SelectFirst,
    SelectLast, SelectPageDown, SelectPageUp, SelectUp, ToggleSelected,
};
use crate::selection::Move;
use crate::theme::{self, Colors, size, text};
use crate::window::MainWindow;

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.set_scale_factor(window.scale_factor(), window, cx);
        let colors = Colors::of(cx);
        // Keys always land somewhere: in a main window on the window
        // itself, which a click inside takes focus back to; in a picker in
        // its name box, whose keys pass the window's on.
        let main = self.pick.is_none();
        let narrow = window.viewport_size().width < theme::window::NARROW_WIDTH;
        if window.focused(cx).is_none() {
            let focus = self.pick.as_ref().map_or_else(
                || self.focus.clone(),
                |pick| pick.name.read(cx).focus_handle(cx),
            );
            window.focus(&focus, cx);
        }
        div()
            .key_context(commands::KEY_CONTEXT)
            .when(main, |root| root.track_focus(&self.focus))
            .on_action(cx.listener(|this, _: &GoUp, _, _| this.go_up()))
            .on_action(cx.listener(|this, _: &GoBack, _, cx| this.go_back(cx)))
            .on_action(cx.listener(|this, _: &GoForward, _, cx| this.go_forward(cx)))
            .map(|root| Self::selection_keys(root, cx))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if this.jump_typed(event, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.band_moved(event, cx);
            }))
            .on_scroll_wheel(cx.listener(|this, _: &ScrollWheelEvent, _, cx| {
                this.band_scrolled(cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _, cx| this.band_released(event, cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|this, _: &MouseDownEvent, _, cx| this.go_back(cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|this, _: &MouseDownEvent, _, cx| this.go_forward(cx)),
            )
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.window)
            .text_color(colors.text_soft)
            .font_family(text::FAMILY)
            .text_size(text::BODY)
            .font_weight(text::BODY_WEIGHT)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .pt(size::GAP)
                    .pl(size::GAP)
                    .map(|body| {
                        if narrow {
                            body.flex_col()
                        } else {
                            body.gap(size::GAP)
                        }
                    })
                    .child(self.sidebar(narrow, cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(self.top_bar(&colors))
                            .child(self.sheet(cx)),
                    ),
            )
            .children(self.pick.as_ref().map(|pick| Self::pick_bar(pick, cx)))
            .children(self.menu_overlay(cx))
    }
}

impl MainWindow {
    /// The selection's actions on the window's root (see `selecting`).
    fn selection_keys(root: Div, cx: &Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &OpenSelected, _, _| this.open_selected()))
            .on_action(cx.listener(|this, _: &CursorUp, _, cx| {
                this.move_cursor(Move::Up, false, cx);
            }))
            .on_action(cx.listener(|this, _: &CursorDown, _, cx| {
                this.move_cursor(Move::Down, false, cx);
            }))
            .on_action(cx.listener(|this, _: &CursorPageUp, _, cx| {
                this.move_cursor(Move::PageUp, false, cx);
            }))
            .on_action(cx.listener(|this, _: &CursorPageDown, _, cx| {
                this.move_cursor(Move::PageDown, false, cx);
            }))
            .on_action(cx.listener(|this, _: &CursorFirst, _, cx| {
                this.move_cursor(Move::First, false, cx);
            }))
            .on_action(cx.listener(|this, _: &CursorLast, _, cx| {
                this.move_cursor(Move::Last, false, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectUp, _, cx| {
                this.move_cursor(Move::Up, true, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectDown, _, cx| {
                this.move_cursor(Move::Down, true, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectPageUp, _, cx| {
                this.move_cursor(Move::PageUp, true, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectPageDown, _, cx| {
                this.move_cursor(Move::PageDown, true, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectFirst, _, cx| {
                this.move_cursor(Move::First, true, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectLast, _, cx| {
                this.move_cursor(Move::Last, true, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| this.select_all(cx)))
            .on_action(cx.listener(|this, _: &ToggleSelected, _, cx| this.toggle_cursor(cx)))
            .on_action(cx.listener(|this, _: &ClearSelection, _, cx| this.clear_selection(cx)))
    }
}
