use std::path::PathBuf;

use gpui::{
    AnyElement, ClickEvent, Context, Div, Focusable as _, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, ParentElement as _, StatefulInteractiveElement as _,
    Styled as _, Window, div,
};

use super::row::icon;
use crate::assets::Icon;
use crate::commands::CursorDown;
use crate::theme::{Colors, radius, size, text};
use crate::window::{FocusTo, MainWindow, Scope, short_folder};

const ONE_MATCH: &str = "1 match";
const EVERYWHERE: &str = "Everywhere";
const HERE_ONLY: &str = "Here only";
const NO_MATCHES: &str = "No matches";
const CANT_SEARCH: &str = "Can't search";
const SEARCH_LABEL: &str = "Search";
const SEARCH_KEY: &str = "/";

impl MainWindow {
    pub(super) fn search_box(&self, capsule: Div, colors: &Colors, cx: &Context<Self>) -> Div {
        if self.pick.is_some() {
            return capsule
                .text_color(colors.text_dim)
                .child(icon(Icon::Search, size::BUTTON_ICON, colors.text_dim))
                .child(div().min_w_0().truncate().child(SEARCH_LABEL));
        }
        capsule
            .on_action(cx.listener(|this, _: &CursorDown, _, cx| {
                this.focus_later(FocusTo::Results, cx);
            }))
            .child(icon(Icon::Search, size::BUTTON_ICON, colors.text_dim))
            .child(
                div()
                    .debug_selector(|| "search-box".to_owned())
                    .flex_1()
                    .min_w_0()
                    .text_color(colors.text)
                    .child(self.search.field.clone()),
            )
    }

    pub(super) fn scope_chip(&self, colors: &Colors, cx: &Context<Self>) -> Option<AnyElement> {
        if !self.searching() {
            return None;
        }
        let (label, other) = match self.scope() {
            Scope::Everywhere => (EVERYWHERE, Scope::HereOnly),
            Scope::HereOnly => (HERE_ONLY, Scope::Everywhere),
        };
        Some(
            div()
                .id("scope")
                .debug_selector(|| "scope".to_owned())
                .flex()
                .flex_none()
                .items_center()
                .h(size::CHIP_HEIGHT)
                .px(size::CHIP_PAD)
                .rounded(radius::ROW)
                .bg(colors.press)
                .text_size(text::LABEL)
                .font_weight(text::DETAIL_WEIGHT)
                .text_color(colors.text_soft)
                .cursor_pointer()
                .hover(|chip| chip.bg(colors.selected).text_color(colors.text))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.set_scope(other, cx);
                }))
                .child(label)
                .into_any_element(),
        )
    }

    pub(super) fn folder_link(
        &self,
        path: PathBuf,
        index: usize,
        cx: &Context<Self>,
    ) -> AnyElement {
        let colors = Colors::of(cx);
        let folder = path.parent().unwrap_or(&path);
        let label = short_folder(folder, &self.home);
        div()
            .id(("folder", index))
            .debug_selector(move || format!("folder-{index}"))
            .max_w_full()
            .h(size::FOLDER_LINK_HEIGHT)
            .flex()
            .items_center()
            .px(size::FOLDER_LINK_PAD)
            .ml(-size::FOLDER_LINK_PAD)
            .rounded(radius::SMALL)
            .text_size(text::DETAIL)
            .font_weight(text::DETAIL_WEIGHT)
            .text_color(colors.text_dim)
            .cursor_pointer()
            .hover(|link| link.bg(colors.press).text_color(colors.text))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, _| this.pressed_name = true),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                this.reveal(path.clone());
                cx.notify();
            }))
            .child(div().min_w_0().truncate().child(label))
            .into_any_element()
    }

    pub(super) fn match_count(&self, colors: &Colors) -> Option<impl IntoElement> {
        let found = self.found_count().filter(|&count| count > 0)?;
        let label = match_label(found);
        Some(
            div()
                .debug_selector(|| "match-count".to_owned())
                .absolute()
                .left(size::STATUS_INSET)
                .bottom(size::STATUS_INSET)
                .px(size::STATUS_PAD_X)
                .py(size::STATUS_PAD_Y)
                .rounded(radius::SMALL)
                .bg(colors.sheet)
                .text_size(text::LABEL)
                .font_weight(text::DETAIL_WEIGHT)
                .text_color(colors.text_dim)
                .child(label),
        )
    }

    pub(super) fn no_matches(&self, colors: &Colors) -> Option<impl IntoElement> {
        let label = if self.search.failed {
            CANT_SEARCH
        } else {
            NO_MATCHES
        };
        (self.found_count() == Some(0)).then(|| {
            div()
                .debug_selector(|| "no-matches".to_owned())
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(size::EMPTY_GAP)
                .text_color(colors.text_dim)
                .child(icon(Icon::Search, size::EMPTY_ICON, colors.text_faint))
                .child(label)
        })
    }

    fn found_count(&self) -> Option<usize> {
        let listing = self.listing.as_ref()?;
        (self.searching() && listing.folders.is_some()).then(|| listing.entries.len())
    }

    pub(super) fn search_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let held = event.keystroke.modifiers;
        let plain = !(held.control || held.alt || held.platform || held.function);
        let slash = event.keystroke.key_char.as_deref() == Some(SEARCH_KEY);
        if self.pick.is_none() && plain && slash {
            self.focus_later(FocusTo::Box, cx);
            return true;
        }
        false
    }

    pub(super) fn settle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.search.focus_to.take() {
            Some(FocusTo::Box) => {
                let field = self.search.field.clone();
                field.update(cx, |field, cx| {
                    let all = 0..field.text().len();
                    field.select_range(all, cx);
                });
                window.focus(&field.focus_handle(cx), cx);
            }
            Some(FocusTo::Results) => {
                window.focus(&self.focus, cx);
                if self.cursor().is_none() {
                    self.move_cursor(crate::selection::Move::Down, false, cx);
                }
            }
            None => {}
        }
    }
}

fn match_label(found: usize) -> String {
    if found == 1 {
        ONE_MATCH.to_owned()
    } else {
        format!("{found} matches")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_match_is_singular() {
        assert_eq!(match_label(1), "1 match");
        assert_eq!(match_label(2), "2 matches");
    }
}
