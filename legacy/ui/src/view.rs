mod bar;
mod drop;
mod gallery;
mod icons;
mod island;
mod menu;
mod path;
mod preview;
mod row;
mod search;
mod sheet;
mod sidebar;
mod start;

pub use row::{RowIcon, entry_kind};

use gpui::{
    App, Context, Div, FocusHandle, InteractiveElement as _, IntoElement, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, NavigationDirection,
    ParentElement as _, Render, ScrollWheelEvent, Styled as _, Window, div,
    prelude::FluentBuilder as _,
};
use nova_files_delete_ui::DeleteSelected;
use nova_files_folders_api::ViewMode;

use crate::commands::{
    self, ClearSelection, CursorDown, CursorFirst, CursorLast, CursorPageDown, CursorPageUp,
    CursorUp, FocusSearch, GoBack, GoForward, GoUp, OpenMenu, OpenSelected, PreviewNext,
    PreviewPrevious, SelectAll, SelectDown, SelectFirst, SelectLast, SelectLeft, SelectPageDown,
    SelectPageUp, SelectRight, SelectUp, TogglePreview, ToggleSelected,
};
use crate::commands::{BiggerIcons, ShowDetails, ShowGallery, ShowIcons, SmallerIcons};
use crate::commands::{CloseTab, NewTab, NextTab, PreviousTab, ShowProperties, TrashSelected};
use crate::selection::Move;
use crate::theme::{self, Colors, size, text};
use crate::window::{FocusTo, MainWindow};

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.closing(cx) {
            window.remove_window();
        }
        let scale_factor = window.scale_factor();
        let viewport = window.viewport_size();
        self.keep_gallery(window, cx);
        let room = preview::room(viewport.width, viewport.height);
        let fits = if self.showing_gallery() && !self.previewing() {
            self.stage_room()
        } else {
            room
        };
        self.set_preview_room(fits, scale_factor, cx);
        self.window_root(window, cx)
            .map(|root| Self::preview_keys(root, cx))
            .relative()
            .children(self.preview_layer(room, scale_factor, cx))
    }
}

impl MainWindow {
    fn window_root(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        self.set_scale_factor(window.scale_factor(), window, cx);
        self.settle_search(window, cx);
        self.settle_tab_focus(window, cx);
        let colors = Colors::of(cx);
        let main = !self.picker();
        let narrow = window.viewport_size().width < theme::window::NARROW_WIDTH;
        if window.focused(cx).is_none() {
            window.focus(&self.keys_home(cx), cx);
        }
        self.focus_menu(window, cx);
        div()
            .key_context(commands::KEY_CONTEXT)
            .on_modifiers_changed(|_, window, cx| {
                if cx.has_active_drag() {
                    window.refresh();
                }
            })
            .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, window, cx| {
                this.pointer_moved(cx.has_active_drag(), window, cx);
            }))
            .when(main, |root| root.track_focus(&self.focus))
            .on_action(cx.listener(|this, _: &GoUp, _, _| this.go_up()))
            .on_action(cx.listener(|this, _: &GoBack, _, cx| this.go_back(cx)))
            .on_action(cx.listener(|this, _: &GoForward, _, cx| this.go_forward(cx)))
            .map(|root| Self::view_keys(root, cx))
            .on_action(cx.listener(|this, _: &OpenMenu, _, cx| this.menu_key(cx)))
            .map(|root| Self::typed_keys(root, main, cx))
            .map(|root| Self::selection_keys(root, cx))
            .map(|root| Self::file_keys(root, cx))
            .map(|root| self.parts.bind(root))
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
            .bg(colors.sheet)
            .text_color(colors.text_soft)
            .font_family(text::FAMILY)
            .text_size(text::BODY)
            .font_weight(text::BODY_WEIGHT)
            .child(self.body(narrow, &colors, cx))
            .children(self.parts.footers())
            .children(self.menu_overlay(window, cx))
    }
}

impl MainWindow {
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

    fn view_keys(root: Div, cx: &Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &ShowDetails, _, cx| {
            this.show_as(ViewMode::Details, cx);
        }))
        .on_action(cx.listener(|this, _: &ShowIcons, _, cx| this.show_as(ViewMode::Icons, cx)))
        .on_action(cx.listener(|this, _: &ShowGallery, _, cx| {
            this.show_as(ViewMode::Gallery, cx);
        }))
        .on_action(cx.listener(|this, _: &BiggerIcons, _, cx| this.zoom_icons(true, cx)))
        .on_action(cx.listener(|this, _: &SmallerIcons, _, cx| this.zoom_icons(false, cx)))
    }

    fn tab_keys(root: Div, cx: &Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &NewTab, _, cx| {
            this.new_tab(&crate::location::Location::Start, cx);
        }))
        .on_action(cx.listener(|this, _: &CloseTab, _, cx| this.close_current_tab(cx)))
        .on_action(cx.listener(|this, _: &NextTab, _, cx| this.next_tab(cx)))
        .on_action(cx.listener(|this, _: &PreviousTab, _, cx| this.previous_tab(cx)))
        .on_action(cx.listener(|this, _: &ShowProperties, _, cx| this.show_properties(cx)))
    }

    fn file_keys(root: Div, cx: &Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &TrashSelected, window, cx| {
            if this.folder_rules().trashed {
                window.dispatch_action(Box::new(DeleteSelected), cx);
            } else {
                this.trash_selected(cx);
            }
        }))
    }

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
    fn typed_keys(root: Div, main: bool, cx: &Context<Self>) -> Div {
        root.when(main, |root| {
            root.on_action(cx.listener(|this, _: &FocusSearch, _, cx| {
                this.focus_later(FocusTo::Box, cx);
            }))
        })
        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
            if !this.focus.is_focused(window) {
                return;
            }
            if this.search_key(event, cx) || this.jump_typed(event, cx) {
                cx.stop_propagation();
            }
        }))
    }

    pub fn keys_home(&self, cx: &App) -> FocusHandle {
        self.parts.focus(cx).unwrap_or_else(|| self.focus.clone())
    }

    fn selection_keys(root: Div, cx: &Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &OpenSelected, window, cx| {
            if !this.open_previewed(window, cx) {
                this.open_selected(cx);
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
        .on_action(cx.listener(|this, _: &SelectLeft, _, cx| {
            if this.showing_icons() {
                this.move_cursor(Move::Left, true, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &SelectRight, _, cx| {
            if this.showing_icons() {
                this.move_cursor(Move::Right, true, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &SelectAll, _, cx| this.select_all(cx)))
        .on_action(cx.listener(|this, _: &ToggleSelected, _, cx| this.toggle_cursor(cx)))
        .on_action(cx.listener(|this, _: &ClearSelection, window, cx| {
            if this.previewing() {
                this.dismiss_preview(window, cx);
            } else {
                this.clear_selection(cx);
            }
        }))
    }

    fn preview_keys(root: Div, cx: &Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &TogglePreview, window, cx| {
            if !this.focus.is_focused(window) || this.jumping(cx) {
                cx.propagate();
            } else {
                this.toggle_preview(window, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &PreviewPrevious, _, cx| {
            this.step_sideways(Move::Left, cx);
        }))
        .on_action(cx.listener(|this, _: &PreviewNext, _, cx| {
            this.step_sideways(Move::Right, cx);
        }))
    }
}
