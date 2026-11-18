use gpui::{
    AnyElement, ClickEvent, Context, Corner, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, anchored,
    deferred, div, prelude::FluentBuilder as _,
};
use nova_files_api::{Entry, EntryKind, FilterRule, PickRequest};

use super::{Mode, Picker};
use crate::theme::{Colors, radius, size};
use crate::window::MainWindow;

const ALL_FILES_LABEL: &str = "All files";
const EVERY_NAME: &str = "*";
const MENU_PRIORITY: usize = 1;

pub fn first(request: &PickRequest) -> Option<usize> {
    let count = request.filters.len();
    request
        .filter
        .filter(|&filter| filter < count)
        .or_else(|| (count > 0).then_some(0))
}

pub fn offers_all(request: &PickRequest) -> bool {
    request.filters.iter().any(|filter| {
        filter
            .rules
            .iter()
            .any(|rule| matches!(rule, FilterRule::Glob(glob) if glob == EVERY_NAME))
    })
}

pub fn label(request: &PickRequest, filter: Option<usize>) -> SharedString {
    filter
        .and_then(|filter| request.filters.get(filter))
        .map_or_else(
            || ALL_FILES_LABEL.into(),
            |filter| filter.name.clone().into(),
        )
}

pub fn is_chosen(chosen: Option<usize>, filter: Option<usize>) -> bool {
    chosen == filter
}

impl Picker {
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
}

impl MainWindow {
    pub fn pick_filter(&self) -> Option<usize> {
        self.pick.as_ref()?.filter
    }

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

    pub fn filter_button(pick: &Picker, cx: &Context<Self>) -> Option<impl IntoElement> {
        if pick.request.filters.is_empty() || pick.mode == Mode::Folders {
            return None;
        }
        let colors = Colors::of(cx);
        let button = super::bar::button(
            "pick-filter",
            label(&pick.request, pick.filter),
            false,
            &colors,
        )
        .flex_shrink()
        .min_w_0()
        .max_w_full()
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
            div().absolute().top_0().left_0().child(
                deferred(
                    anchored()
                        .anchor(Corner::BottomLeft)
                        .snap_to_window()
                        .child(
                            div()
                                .id("pick-filter-menu")
                                .debug_selector(|| "pick-filter-menu".to_owned())
                                .flex()
                                .flex_col()
                                .max_h(size::FILTER_MENU_MAX_HEIGHT)
                                .overflow_y_scroll()
                                .p(size::MENU_PAD)
                                .rounded(radius::CARD)
                                .border(size::LINE)
                                .border_color(colors.line)
                                .bg(colors.lifted)
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
        Some(
            div()
                .relative()
                .flex()
                .flex_shrink()
                .min_w_0()
                .max_w(size::FILTER_MAX_WIDTH)
                .child(button)
                .children(menu),
        )
    }
}

fn menu_item(pick: &Picker, filter: Option<usize>, cx: &Context<MainWindow>) -> AnyElement {
    let selector = filter.map_or_else(
        || "pick-filter-all".to_owned(),
        |filter| format!("pick-filter-{filter}"),
    );
    let colors = Colors::of(cx);
    div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .flex()
        .items_center()
        .h(size::MENU_ROW_HEIGHT)
        .px(size::PLACE_PAD)
        .rounded(radius::ROW)
        .cursor_pointer()
        .when(is_chosen(pick.filter, filter), |item| {
            item.bg(colors.selected)
        })
        .hover(|item| item.bg(colors.hover))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.choose_filter(filter);
            cx.notify();
        }))
        .child(label(&pick.request, filter))
        .into_any_element()
}
