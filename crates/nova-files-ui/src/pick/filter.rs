//! The picker's type filter: the app's file types, plus All files unless
//! one of them already takes every file, in a
//! menu that opens up from the pick bar. Files the chosen type leaves out
//! are dimmed, not hidden, and can still be picked; a folder picker dims
//! every file. Each menu item carries a debug selector: `pick-filter`
//! opens the menu, `pick-filter-<n>` (counted from 0) and
//! `pick-filter-all` choose.

use gpui::{
    AnyElement, ClickEvent, Context, Corner, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, anchored,
    deferred, div, prelude::FluentBuilder as _, rgb,
};
use nova_files_api::{Entry, EntryKind, FilterRule, PickRequest};

use super::{Mode, Picker};
use crate::theme::{color, radius, size};
use crate::window::MainWindow;

const ALL_FILES_LABEL: &str = "All files";
/// The glob that matches every name.
const EVERY_NAME: &str = "*";
/// The menu draws over the list.
const MENU_PRIORITY: usize = 1;

/// The filter a picker starts on: the one the app asks for, or else its
/// first, as GTK's dialog does. `None` is All files.
pub fn first(request: &PickRequest) -> Option<usize> {
    let count = request.filters.len();
    request
        .filter
        .filter(|&filter| filter < count)
        .or_else(|| (count > 0).then_some(0))
}

/// Whether one of the app's own types already takes every file, as
/// browsers' "All Files" does; then the menu adds no All files of its own.
pub fn offers_all(request: &PickRequest) -> bool {
    request.filters.iter().any(|filter| {
        filter
            .rules
            .iter()
            .any(|rule| matches!(rule, FilterRule::Glob(glob) if glob == EVERY_NAME))
    })
}

/// What the menu and its button call `filter`: the app's name for it, or
/// All files.
pub fn label(request: &PickRequest, filter: Option<usize>) -> SharedString {
    filter
        .and_then(|filter| request.filters.get(filter))
        .map_or_else(
            || ALL_FILES_LABEL.into(),
            |filter| filter.name.clone().into(),
        )
}

/// Whether the menu item for `filter` shows as the one `chosen`.
pub fn is_chosen(chosen: Option<usize>, filter: Option<usize>) -> bool {
    chosen == filter
}

impl Picker {
    /// Whether the chosen type leaves `entry` out.
    pub fn dims(&self, entry: &Entry) -> bool {
        if entry.kind != EntryKind::File {
            return false;
        }
        if self.mode == Mode::Folders {
            return true;
        }
        self.filter
            .and_then(|filter| self.request.filters.get(filter))
            .is_some_and(|filter| !filter.admits(&entry.name, self.request.types.as_deref()))
    }

    fn filter_label(&self, filter: Option<usize>) -> SharedString {
        label(&self.request, filter)
    }
}

impl MainWindow {
    /// The type filter chosen, as an index into the app's filters; `None`
    /// for All files.
    pub fn pick_filter(&self) -> Option<usize> {
        self.pick.as_ref()?.filter
    }

    /// Whether a picker leaves the entry at `index` on screen out.
    pub fn pick_dims(&self, index: usize) -> bool {
        let entry = self
            .listing
            .as_ref()
            .and_then(|listing| listing.entries.get(index));
        self.pick
            .as_ref()
            .zip(entry)
            .is_some_and(|(pick, entry)| pick.dims(entry))
    }

    const fn toggle_filter_menu(&mut self) {
        if let Some(pick) = &mut self.pick {
            pick.filter_menu = !pick.filter_menu;
        }
    }

    const fn choose_filter(&mut self, filter: Option<usize>) {
        if let Some(pick) = &mut self.pick {
            pick.filter = filter;
            pick.filter_menu = false;
        }
    }

    /// The filter button, and its menu while open. Nothing when the app
    /// offers no types, or for a folder picker.
    pub fn filter_button(pick: &Picker, cx: &Context<Self>) -> Option<impl IntoElement> {
        if pick.request.filters.is_empty() || pick.mode == Mode::Folders {
            return None;
        }
        let button = super::bar::button("pick-filter", pick.filter_label(pick.filter), false)
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.toggle_filter_menu();
                cx.notify();
            }));
        let menu = pick.filter_menu.then(|| {
            let all = (!offers_all(&pick.request)).then_some(None);
            let items: Vec<AnyElement> = (0..pick.request.filters.len())
                .map(Some)
                .chain(all)
                .map(|filter| menu_item(pick, filter, cx))
                .collect();
            // Pinned to the button's top left, opening upward.
            div().absolute().top_0().left_0().child(
                deferred(
                    anchored()
                        .anchor(Corner::BottomLeft)
                        .snap_to_window()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .p(size::ROW_GAP)
                                .rounded(radius::BUTTON)
                                .border(size::DIVIDER)
                                .border_color(rgb(color::DIVIDER))
                                .bg(rgb(color::SURFACE))
                                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                    if let Some(pick) = &mut this.pick {
                                        pick.filter_menu = false;
                                    }
                                    cx.notify();
                                }))
                                .children(items),
                        ),
                )
                .with_priority(MENU_PRIORITY),
            )
        });
        Some(div().relative().flex_none().child(button).children(menu))
    }
}

fn menu_item(pick: &Picker, filter: Option<usize>, cx: &Context<MainWindow>) -> AnyElement {
    let selector = filter.map_or_else(
        || "pick-filter-all".to_owned(),
        |filter| format!("pick-filter-{filter}"),
    );
    div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .flex()
        .items_center()
        .h(size::ROW_HEIGHT)
        .px(size::PAD)
        .rounded(radius::ROW)
        .cursor_pointer()
        .when(is_chosen(pick.filter, filter), |item| {
            item.bg(rgb(color::ROW_SELECTED))
        })
        .hover(|item| item.bg(rgb(color::ROW_HIGHLIGHT)))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.choose_filter(filter);
            cx.notify();
        }))
        .child(pick.filter_label(filter))
        .into_any_element()
}
