use gpui::{
    Anchor, AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    anchored, deferred, div, prelude::FluentBuilder as _,
};
use nova_files_shell::Scene;
use nova_files_shell::theme::{Colors, radius, size, text};

use crate::filter::{is_chosen, label, offers_all};
use crate::part::{PickerPart, Picking};
use crate::{Mode, accept_label};

const CANCEL_LABEL: &str = "Cancel";
const REPLACE_LABEL: &str = "Replace";
const KEEP_BOTH_LABEL: &str = "Keep both";
const MENU_PRIORITY: usize = 1;

impl PickerPart {
    pub(crate) fn footer(
        &mut self,
        _: &Scene,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let pick = self.pick.as_ref()?;
        Some(bar(pick, cx).into_any_element())
    }
}

fn bar(pick: &Picking, cx: &Context<PickerPart>) -> impl IntoElement {
    let colors = Colors::of(cx);
    let name = div()
        .debug_selector(|| "pick-name".to_owned())
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .h(size::CONTROL_HEIGHT)
        .px(size::PLACE_PAD)
        .rounded(radius::CONTROL)
        .bg(colors.raised)
        .child(pick.name.clone());
    let controls = div()
        .flex()
        .items_center()
        .gap(size::GAP)
        .child(name)
        .children(filter_button(pick, cx))
        .child(
            button("pick-cancel", CANCEL_LABEL.into(), false, &colors)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.cancel(cx))),
        )
        .child(
            button(
                "pick-accept",
                accept_label(&pick.request).into(),
                true,
                &colors,
            )
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.accept(cx))),
        );
    let question = pick.conflict.as_ref().map(|conflict| {
        let keep_both = format!(
            "{KEEP_BOTH_LABEL} ({})",
            conflict.keep_both.to_string_lossy()
        );
        div()
            .flex()
            .items_center()
            .justify_end()
            .gap(size::GAP)
            .child(
                button("pick-replace", REPLACE_LABEL.into(), false, &colors)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.replace(cx))),
            )
            .child(
                button("pick-keep-both", keep_both.into(), true, &colors).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.keep_both(cx);
                        cx.notify();
                    },
                )),
            )
    });
    div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(size::QUESTION_GAP)
        .p(size::GAP)
        .bg(colors.window)
        .child(controls)
        .children(question)
}

fn filter_button(pick: &Picking, cx: &Context<PickerPart>) -> Option<impl IntoElement> {
    if pick.request.filters.is_empty() || pick.mode == Mode::Folders {
        return None;
    }
    let colors = Colors::of(cx);
    let button = button(
        "pick-filter",
        label(&pick.request, pick.filter),
        false,
        &colors,
    )
    .flex_shrink_1()
    .min_w_0()
    .max_w_full()
    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
        if let Some(pick) = &mut this.pick {
            pick.filter_menu = !pick.filter_menu;
        }
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
                    .anchor(Anchor::BottomLeft)
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
            .flex_shrink_1()
            .min_w_0()
            .max_w(size::FILTER_MAX_WIDTH)
            .child(button)
            .children(menu),
    )
}

fn menu_item(pick: &Picking, filter: Option<usize>, cx: &Context<PickerPart>) -> AnyElement {
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
            if let Some(pick) = &mut this.pick {
                pick.filter = filter;
                pick.filter_menu = false;
            }
            cx.notify();
        }))
        .child(label(&pick.request, filter))
        .into_any_element()
}

fn button(
    id: &'static str,
    label: SharedString,
    accept: bool,
    colors: &Colors,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .debug_selector(move || id.to_owned())
        .flex()
        .flex_none()
        .items_center()
        .h(size::CONTROL_HEIGHT)
        .px(size::CONTROL_PAD)
        .rounded(radius::CONTROL)
        .font_weight(text::STRONG_WEIGHT)
        .cursor_pointer()
        .map(|button| {
            if accept {
                button.bg(colors.accept).text_color(colors.on_accept)
            } else {
                button
                    .bg(colors.raised)
                    .hover(|button| button.bg(colors.lifted))
            }
        })
        .child(div().min_w_0().truncate().child(label))
}
