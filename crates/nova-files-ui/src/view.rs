//! Draws the main window as on the design page: the sidebar and the top
//! bar sit on the window itself, and the folder's entries are one sheet
//! that runs to the window's right and bottom edges, with a rounded corner
//! where the sidebar and the bar meet. In pick mode the pick bar runs along
//! the bottom (see `pick`).
//!
//! Each part is drawn in a file of its own, so a feature changes only its
//! part: the top bar in `bar`, its path bar in `path`, the sidebar in
//! `sidebar`, the sheet with its column heads and rows in `sheet`, the
//! pieces of one row in `row`, and the quick look over it all in `preview`.
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
//! `path`, `search` and `views`; the path bar's own are listed in `path`.

mod bar;
mod path;
mod path_menu;
mod preview;
mod row;
mod sheet;
mod sidebar;

pub use sidebar::place_icon;

use gpui::{
    App, Context, Div, FocusHandle, Focusable as _, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, NavigationDirection,
    ParentElement as _, Render, ScrollWheelEvent, Styled as _, Window, div,
    prelude::FluentBuilder as _,
};

use crate::commands::{
    self, ClearSelection, CursorDown, CursorFirst, CursorLast, CursorPageDown, CursorPageUp,
    CursorUp, GoBack, GoForward, GoUp, OpenSelected, PreviewNext, PreviewPrevious, SelectAll,
    SelectDown, SelectFirst, SelectLast, SelectPageDown, SelectPageUp, SelectUp, TogglePreview,
    ToggleSelected,
};
use crate::selection::Move;
use crate::theme::{Colors, size, text};
use crate::window::MainWindow;

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let scale_factor = window.scale_factor();
        let viewport = window.viewport_size();
        let room = preview::room(viewport.width, viewport.height);
        self.set_preview_room(room, scale_factor, cx);
        self.window_root(window, cx)
            .map(|root| Self::preview_keys(root, cx))
            .relative()
            .children(self.preview_layer(room, scale_factor, cx))
    }
}

impl MainWindow {
    /// The window without the quick look over it.
    fn window_root(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        self.set_scale_factor(window.scale_factor(), window, cx);
        let colors = Colors::of(cx);
        // Keys always land somewhere (see `keys_home`).
        let main = self.pick.is_none();
        if window.focused(cx).is_none() {
            window.focus(&self.keys_home(cx), cx);
        }
        div()
            .key_context(commands::KEY_CONTEXT)
            .when(main, |root| root.track_focus(&self.focus))
            .on_action(cx.listener(|this, _: &GoUp, _, _| this.go_up()))
            .on_action(cx.listener(|this, _: &GoBack, _, _| this.go_back()))
            .on_action(cx.listener(|this, _: &GoForward, _, _| this.go_forward()))
            .map(|root| Self::selection_keys(root, cx))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                // Letters typed into a text field are the field's.
                if this.focus.is_focused(window) && this.jump_typed(event, cx) {
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
                cx.listener(|this, _: &MouseDownEvent, _, _| this.go_back()),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|this, _: &MouseDownEvent, _, _| this.go_forward()),
            )
            .size_full()
            .flex()
            .flex_col()
            // The sheet's color fills the window once, and the window's
            // own color is painted only around the sheet: a software
            // renderer pays for every layer it fills.
            .bg(colors.sheet)
            .text_color(colors.text_soft)
            .font_family(text::FAMILY)
            .text_size(text::BODY)
            .font_weight(text::BODY_WEIGHT)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_none()
                            .px(size::GAP)
                            .pt(size::GAP)
                            .bg(colors.window)
                            .child(self.sidebar(cx)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex_none()
                                    .pt(size::GAP)
                                    .bg(colors.window)
                                    .child(self.top_bar(&colors, cx)),
                            )
                            .child(self.sheet(cx)),
                    ),
            )
            .children(self.pick.as_ref().map(|pick| Self::pick_bar(pick, cx)))
    }

    /// Where the keys go when nothing else holds them: in a main window
    /// the window itself, which a click inside takes focus back to; in a
    /// picker its name box, whose keys pass the window's on.
    pub fn keys_home(&self, cx: &App) -> FocusHandle {
        self.pick.as_ref().map_or_else(
            || self.focus.clone(),
            |pick| pick.name.read(cx).focus_handle(cx),
        )
    }

    /// The selection's actions on the window's root (see `selecting`).
    fn selection_keys(root: Div, cx: &Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &OpenSelected, window, cx| {
            if !this.open_previewed(window, cx) {
                this.open_selected();
            }
        }))
        .on_action(cx.listener(|this, _: &CursorUp, _, cx| {
            if !this.step_preview(Move::Up, cx) {
                this.move_cursor(Move::Up, false, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &CursorDown, _, cx| {
            if !this.step_preview(Move::Down, cx) {
                this.move_cursor(Move::Down, false, cx);
            }
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
        .on_action(cx.listener(|this, _: &ClearSelection, window, cx| {
            if this.previewing() {
                this.close_preview(Some(window), cx);
            } else {
                this.clear_selection(cx);
            }
        }))
    }

    /// The quick look's actions on the window's root (see
    /// `window::preview`). A key it doesn't take goes on as typed: a space
    /// inside a name being jumped to stays part of the name, and in a
    /// picker Space goes to its name box. (The name box takes the arrows
    /// itself before they get here.)
    fn preview_keys(root: Div, cx: &Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &TogglePreview, window, cx| {
            if this.pick.is_some() || this.jumping(cx) {
                cx.propagate();
            } else {
                this.toggle_preview(window, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &PreviewPrevious, _, cx| {
            this.step_preview(Move::Up, cx);
        }))
        .on_action(cx.listener(|this, _: &PreviewNext, _, cx| {
            this.step_preview(Move::Down, cx);
        }))
    }
}
