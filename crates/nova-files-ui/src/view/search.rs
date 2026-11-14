//! The search's parts of the window: the box in the top bar, the count
//! of files found at the sheet's bottom left, and what shows when nothing
//! was found. The box's keys are handled here too: Down in it moves into
//! the results, and the window's own keys (`/`, Ctrl+F) bring the caret
//! back (see `window/search`).

use gpui::{
    App, Context, Div, Focusable as _, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Styled as _, Window, div,
};

use super::row::icon;
use crate::assets::Icon;
use crate::commands::CursorDown;
use crate::theme::{Colors, radius, size, text};
use crate::window::{FocusTo, MainWindow};

const ONE_MATCH: &str = "1 match";
const NO_MATCHES: &str = "No matches";
/// Typed outside a text field, it puts the caret in the search box.
const SEARCH_KEY: &str = "/";

impl MainWindow {
    /// The search capsule's contents: the icon and the box. Down in the
    /// box moves into the results.
    pub(super) fn search_box(&self, capsule: Div, colors: &Colors, cx: &Context<Self>) -> Div {
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

    /// How many files the search found, over the sheet's bottom left;
    /// nothing while no search is under way or none were found.
    pub(super) fn match_count(&self, colors: &Colors) -> Option<impl IntoElement> {
        let found = self.found_count().filter(|&count| count > 0)?;
        let label = if found == 1 {
            ONE_MATCH.to_owned()
        } else {
            format!("{found} matches")
        };
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

    /// A quiet icon and "No matches" in the middle of the sheet, when a
    /// search found nothing.
    pub(super) fn no_matches(&self, colors: &Colors) -> Option<impl IntoElement> {
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
                .child(NO_MATCHES)
        })
    }

    /// How many files the search on screen found; `None` while the list
    /// shows a folder, or the first answer hasn't come.
    fn found_count(&self) -> Option<usize> {
        let listing = self.listing.as_ref()?;
        (self.searching() && listing.folders.is_some()).then(|| listing.entries.len())
    }

    /// A key typed while no text field has the keys: `/` puts the caret
    /// in the search box. Whether it was taken.
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

    /// Whether the search box has the keys.
    pub(super) fn search_focused(&self, window: &Window, cx: &App) -> bool {
        self.search
            .field
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
    }

    /// Moves the keyboard where the search asked it to go, now that there
    /// is a window, and empties a box the search left behind.
    pub(super) fn settle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.search.stale_text) {
            self.search
                .field
                .update(cx, |field, cx| field.set_text("", cx));
        }
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
