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
//! part: the top bar in `bar`, its path bar in `path`, the sidebar in
//! `sidebar`, the sheet with its column heads and rows in `sheet`, the
//! pieces of one row in `row`, a right-click menu in `menu`, the Start
//! page, drawn on the sheet in place of the rows, in `start`, and the
//! quick look over it all in `preview`.
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
//! `path`, `search` and `views`; the search's parts carry `search-box`,
//! `match-count` and `no-matches`, and the path bar's own are listed in
//! `path`. The island over the bottom of the sheet is drawn in `island`.

mod bar;
mod drop;
mod island;
mod menu;
mod path;
mod path_menu;
mod preview;
mod row;
mod search;
mod sheet;
mod sidebar;
mod start;

pub use crate::places::place_icon;
pub use row::{RowIcon, entry_kind};

use gpui::{
    App, Context, Div, FocusHandle, Focusable as _, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, NavigationDirection,
    ParentElement as _, Render, ScrollWheelEvent, Styled as _, Window, div,
    prelude::FluentBuilder as _,
};

use crate::commands::{
    self, ClearSelection, CursorDown, CursorFirst, CursorLast, CursorPageDown, CursorPageUp,
    CursorUp, FocusSearch, GoBack, GoForward, GoUp, OpenMenu, OpenSelected, PreviewNext,
    PreviewPrevious, SelectAll, SelectDown, SelectFirst, SelectLast, SelectPageDown, SelectPageUp,
    SelectUp, ToggleHidden, TogglePreview, ToggleSelected,
};
use crate::commands::{
    CloseTab, CopySelected, CutSelected, DeleteSelected, EmptyTrash, NewFolder, NewTab, NextTab,
    Paste, PreviousTab, RenameSelected, RestoreSelected, TrashSelected, Undo,
};
use crate::selection::Move;
use crate::theme::{self, Colors, size, text};
use crate::window::{FocusTo, MainWindow};

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
        self.start_wanted_rename(window, cx);
        self.settle_search(window, cx);
        self.settle_tab_focus(window, cx);
        let colors = Colors::of(cx);
        // Keys always land somewhere (see `keys_home`).
        let main = self.pick.is_none();
        let narrow = window.viewport_size().width < theme::window::NARROW_WIDTH;
        if window.focused(cx).is_none() {
            window.focus(&self.keys_home(cx), cx);
        }
        self.focus_menu(window, cx);
        div()
            .key_context(commands::KEY_CONTEXT)
            // The drag chip says what Ctrl or Shift makes of a drop.
            .on_modifiers_changed(|_, window, cx| {
                if cx.has_active_drag() {
                    window.refresh();
                }
            })
            // After every row under the pointer (see `hover_open`).
            .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, window, cx| {
                this.pointer_moved(cx.has_active_drag(), window, cx);
            }))
            .when(main, |root| root.track_focus(&self.focus))
            .on_action(cx.listener(|this, _: &GoUp, _, _| this.go_up()))
            .on_action(cx.listener(|this, _: &GoBack, _, cx| this.go_back(cx)))
            .on_action(cx.listener(|this, _: &GoForward, _, cx| this.go_forward(cx)))
            .on_action(cx.listener(|this, _: &ToggleHidden, _, _| this.toggle_hidden()))
            .on_action(cx.listener(|this, _: &OpenMenu, _, cx| this.menu_key(cx)))
            .map(|root| Self::typed_keys(root, main, cx))
            .map(|root| Self::selection_keys(root, cx))
            .map(|root| Self::file_keys(root, cx))
            .map(|root| Self::band_events(root, cx))
            .when(main, |root| Self::tab_keys(root, cx))
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
            // The sheet's color fills the window once, and the window's
            // own color is painted only around the sheet: a software
            // renderer pays for every layer it fills.
            .bg(colors.sheet)
            .text_color(colors.text_soft)
            .font_family(text::FAMILY)
            .text_size(text::BODY)
            .font_weight(text::BODY_WEIGHT)
            .child(self.body(narrow, &colors, cx))
            .children(self.pick.as_ref().map(|pick| Self::pick_bar(pick, cx)))
            .children(self.menu_overlay(window, cx))
    }
}

impl MainWindow {
    /// A box being dragged follows the pointer anywhere in the window
    /// (see `band`).
    fn band_events(root: Div, cx: &Context<Self>) -> Div {
        root.on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
            this.band_moved(event, cx);
        }))
        .on_scroll_wheel(cx.listener(|this, _: &ScrollWheelEvent, _, cx| {
            this.band_scrolled(cx);
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, event: &MouseUpEvent, _, cx| this.band_released(event, cx)),
        )
    }

    /// The tabs' actions, in a main window (see `window::tabs`).
    fn tab_keys(root: Div, cx: &Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &NewTab, _, cx| {
            this.new_tab(&crate::location::Location::Start, cx);
        }))
        .on_action(cx.listener(|this, _: &CloseTab, _, cx| this.close_current_tab(cx)))
        .on_action(cx.listener(|this, _: &NextTab, _, cx| this.next_tab(cx)))
        .on_action(cx.listener(|this, _: &PreviousTab, _, cx| this.previous_tab(cx)))
    }

    /// The actions on the files selected (see `window::acting`).
    fn file_keys(root: Div, cx: &Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &CopySelected, _, cx| this.copy_selected(false, cx)))
            .on_action(cx.listener(|this, _: &CutSelected, _, cx| this.copy_selected(true, cx)))
            .on_action(cx.listener(|this, _: &Paste, _, cx| this.paste(cx)))
            .on_action(cx.listener(|this, _: &TrashSelected, _, cx| this.trash_selected(cx)))
            .on_action(cx.listener(|this, _: &DeleteSelected, _, cx| this.delete_selected(cx)))
            .on_action(cx.listener(|this, _: &RestoreSelected, _, _| this.restore_selected()))
            .on_action(cx.listener(|this, _: &EmptyTrash, _, cx| this.ask_empty_trash(cx)))
            .on_action(cx.listener(|this, _: &Undo, _, cx| this.undo(cx)))
            .on_action(cx.listener(|this, _: &RenameSelected, window, cx| {
                this.start_rename(window, cx);
            }))
            .on_action(cx.listener(|this, _: &NewFolder, _, _| this.new_folder()))
    }

    /// The sidebar beside the bar and sheet; above them when `narrow`.
    /// Only the strips around the sheet are painted the window's color.
    fn body(&self, narrow: bool, colors: &Colors, cx: &Context<Self>) -> Div {
        div()
            .flex()
            .flex_1()
            .min_h_0()
            .when(narrow, gpui::Styled::flex_col)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .px(size::GAP)
                    .pt(size::GAP)
                    .bg(colors.window)
                    .child(self.sidebar(narrow, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(
                        div()
                            .flex_none()
                            .pt(size::GAP)
                            .when(narrow, |bar| bar.pl(size::GAP))
                            .bg(colors.window)
                            .child(self.top_bar(colors, cx)),
                    )
                    .child(self.sheet(cx)),
            )
    }
}

impl MainWindow {
    /// What typing does on the window's root: Ctrl+F and `/` go to the
    /// search box in a main window, other letters jump to a name. Keys
    /// typed in a text field are its own.
    fn typed_keys(root: Div, main: bool, cx: &Context<Self>) -> Div {
        root.when(main, |root| {
            root.on_action(cx.listener(|this, _: &FocusSearch, _, cx| {
                this.focus_later(FocusTo::Box, cx);
            }))
        })
        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
            // Letters typed into a text field are the field's.
            if !this.focus.is_focused(window) {
                return;
            }
            if this.search_key(event, cx) || this.jump_typed(event, cx) {
                cx.stop_propagation();
            }
        }))
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
    /// inside a name being jumped to stays part of the name, and one typed
    /// into a text field (a picker's name, a path, a search) stays there.
    /// (A text field takes the arrows itself before they get here.)
    fn preview_keys(root: Div, cx: &Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &TogglePreview, window, cx| {
            if !this.focus.is_focused(window) || this.jumping(cx) {
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
