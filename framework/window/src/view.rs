mod bar;
mod drop;
mod gallery;
mod icons;
mod island;
mod menu;
mod path;
mod row;
mod search;
mod sheet;
mod sidebar;
mod start;

pub use row::{RowIcon, entry_kind};

use gpui::{
    App, Context, DispatchPhase, Div, FocusHandle, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, NavigationDirection,
    ParentElement as _, Render, ScrollEvent, Styled as _, Window, canvas, div,
    prelude::FluentBuilder as _,
};

use crate::commands::{
    self, ClearSelection, CursorDown, CursorFirst, CursorLast, CursorPageDown, CursorPageUp,
    CursorUp, GoBack, GoForward, GoUp, OpenMenu, OpenSelected, PreviewNext, PreviewPrevious,
    SelectAll, SelectDown, SelectFirst, SelectLast, SelectLeft, SelectPageDown, SelectPageUp,
    SelectRight, SelectUp, ToggleSelected,
};
use crate::commands::{
    CloseTab, NewTab, NewWindow, NextTab, PreviousTab, Reload, ReopenTab, RingDown, RingFirst,
    RingLast, RingLeft, RingRight, RingUp, TypePath,
};
use crate::selection::{Move, Reach};
use crate::theme::{self, Colors, size, text};
use crate::window::MainWindow;

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.closing(cx) {
            window.remove_window();
        }
        self.keep_gallery(window, cx);
        self.window_root(window, cx)
            .map(|root| Self::preview_keys(root, cx))
            .relative()
            .children(self.parts.layers())
    }
}

impl MainWindow {
    fn window_root(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        self.set_scale_factor(window.scale_factor(), window, cx);
        self.settle_search(window, cx);
        self.settle_tab_focus(window, cx);
        self.settle_question(window, cx);
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
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                let dragging = cx.has_active_drag();
                this.pointer_moved(dragging, window, cx);
                this.pointer_carried(dragging.then_some(event.position), cx);
            }))
            .when(main, |root| root.track_focus(&self.focus))
            .on_action(cx.listener(|this, _: &GoUp, _, _| this.go_up()))
            .on_action(cx.listener(|this, _: &TypePath, window, cx| this.type_path(window, cx)))
            .on_action(cx.listener(|this, _: &GoBack, _, cx| this.go_back(cx)))
            .on_action(cx.listener(|this, _: &GoForward, _, cx| this.go_forward(cx)))
            .on_action(cx.listener(|this, _: &OpenMenu, _, cx| this.menu_key(cx)))
            .map(|root| Self::typed_keys(root, cx))
            .map(|root| Self::selection_keys(root, cx))
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
            .child(self.body(narrow, &colors, window, cx))
            .children(self.parts.footers())
            .children(self.menu_overlay(window, cx))
    }
}

impl MainWindow {
    fn band_events(root: Div, cx: &Context<Self>) -> Div {
        root.child(pointer_watch(cx))
            .on_scroll(cx.listener(|this, _: &ScrollEvent, _, cx| {
                this.band_scrolled(cx);
            }))
            .capture_any_mouse_down(
                cx.listener(|this, event: &MouseDownEvent, _, _| this.pressed(event)),
            )
            .capture_any_mouse_up(cx.listener(|this, _: &MouseUpEvent, _, cx| {
                this.lift_dropped(cx);
                this.pointer_carried(None, cx);
            }))
    }

    fn tab_keys(root: Div, cx: &Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &NewTab, _, cx| {
            this.new_tab(&nova_files_shell::Location::start(), cx);
        }))
        .on_action(cx.listener(|this, _: &CloseTab, _, cx| this.close_current_tab(cx)))
        .on_action(cx.listener(|this, _: &ReopenTab, _, cx| this.reopen_tab(cx)))
        .on_action(cx.listener(|this, _: &NewWindow, _, cx| this.new_window(cx)))
        .on_action(cx.listener(|this, _: &Reload, _, cx| {
            this.host()
                .nav
                .update(cx, nova_files_shell::Navigator::reload);
        }))
        .on_action(cx.listener(|this, _: &NextTab, _, cx| this.next_tab(cx)))
        .on_action(cx.listener(|this, _: &PreviousTab, _, cx| this.previous_tab(cx)))
    }

    fn body(
        &self,
        narrow: bool,
        colors: &Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let entries =
            (!self.is_on_start() && self.page_shown().is_none()).then(|| self.entries(window, cx));
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
                    .child(self.sheet(entries, cx)),
            )
    }
}

impl MainWindow {
    fn typed_keys(root: Div, cx: &Context<Self>) -> Div {
        root.on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
            if !this.focus.is_focused(window) {
                return;
            }
            let scene = this.scene(cx);
            if this.parts.key_down(event, &scene, cx) || this.jump_typed(event, cx) {
                cx.stop_propagation();
            }
        }))
    }

    pub fn keys_home(&self, cx: &App) -> FocusHandle {
        self.parts.focus(cx).unwrap_or_else(|| self.focus.clone())
    }

    fn selection_keys(root: Div, cx: &Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &OpenSelected, _, cx| {
            this.open_selected(cx);
        }))
        .on_action(cx.listener(|this, _: &CursorUp, _, cx| {
            this.move_cursor(Move::Up, Reach::Only, cx);
        }))
        .on_action(cx.listener(|this, _: &CursorDown, _, cx| {
            this.move_cursor(Move::Down, Reach::Only, cx);
        }))
        .on_action(cx.listener(|this, _: &CursorPageUp, _, cx| {
            this.move_cursor(Move::PageUp, Reach::Only, cx);
        }))
        .on_action(cx.listener(|this, _: &CursorPageDown, _, cx| {
            this.move_cursor(Move::PageDown, Reach::Only, cx);
        }))
        .on_action(cx.listener(|this, _: &CursorFirst, _, cx| {
            this.move_cursor(Move::First, Reach::Only, cx);
        }))
        .on_action(cx.listener(|this, _: &CursorLast, _, cx| {
            this.move_cursor(Move::Last, Reach::Only, cx);
        }))
        .on_action(cx.listener(|this, _: &RingUp, _, cx| {
            this.move_cursor(Move::Up, Reach::Ring, cx);
        }))
        .on_action(cx.listener(|this, _: &RingDown, _, cx| {
            this.move_cursor(Move::Down, Reach::Ring, cx);
        }))
        .on_action(cx.listener(|this, _: &RingFirst, _, cx| {
            this.move_cursor(Move::First, Reach::Ring, cx);
        }))
        .on_action(cx.listener(|this, _: &RingLast, _, cx| {
            this.move_cursor(Move::Last, Reach::Ring, cx);
        }))
        .on_action(cx.listener(|this, _: &RingLeft, _, cx| {
            if this.showing_icons(cx) {
                this.move_cursor(Move::Left, Reach::Ring, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &RingRight, _, cx| {
            if this.showing_icons(cx) {
                this.move_cursor(Move::Right, Reach::Ring, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &SelectUp, _, cx| {
            this.move_cursor(Move::Up, Reach::Extend, cx);
        }))
        .on_action(cx.listener(|this, _: &SelectDown, _, cx| {
            this.move_cursor(Move::Down, Reach::Extend, cx);
        }))
        .on_action(cx.listener(|this, _: &SelectPageUp, _, cx| {
            this.move_cursor(Move::PageUp, Reach::Extend, cx);
        }))
        .on_action(cx.listener(|this, _: &SelectPageDown, _, cx| {
            this.move_cursor(Move::PageDown, Reach::Extend, cx);
        }))
        .on_action(cx.listener(|this, _: &SelectFirst, _, cx| {
            this.move_cursor(Move::First, Reach::Extend, cx);
        }))
        .on_action(cx.listener(|this, _: &SelectLast, _, cx| {
            this.move_cursor(Move::Last, Reach::Extend, cx);
        }))
        .on_action(cx.listener(|this, _: &SelectLeft, _, cx| {
            if this.showing_icons(cx) {
                this.move_cursor(Move::Left, Reach::Extend, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &SelectRight, _, cx| {
            if this.showing_icons(cx) {
                this.move_cursor(Move::Right, Reach::Extend, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &SelectAll, _, cx| this.select_all(cx)))
        .on_action(cx.listener(|this, _: &ToggleSelected, _, cx| this.toggle_cursor(cx)))
        .on_action(cx.listener(|this, _: &ClearSelection, window, cx| {
            if !this.cancel_drag(window, cx) {
                this.clear_selection(cx);
            }
        }))
    }

    fn preview_keys(root: Div, cx: &Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &PreviewPrevious, _, cx| {
            if this.showing_icons(cx) {
                this.move_cursor(Move::Left, Reach::Only, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &PreviewNext, _, cx| {
            if this.showing_icons(cx) {
                this.move_cursor(Move::Right, Reach::Only, cx);
            }
        }))
    }
}

fn pointer_watch(cx: &Context<MainWindow>) -> impl IntoElement {
    let view = cx.entity().downgrade();
    canvas(
        |_, _, _| (),
        move |_, (), window, _| {
            let moved = view.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    moved
                        .update(cx, |this, cx| {
                            this.band_moved(event, cx);
                            this.lift_moved(event, cx);
                        })
                        .ok();
                }
            });
            let released = view;
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                    released
                        .update(cx, |this, cx| this.band_released(event, cx))
                        .ok();
                }
            });
        },
    )
    .absolute()
    .size_0()
}
