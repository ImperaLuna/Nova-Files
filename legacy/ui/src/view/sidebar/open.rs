use gpui::{
    AppContext as _, ClickEvent, Context, Div, DragMoveEvent, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _, div, prelude::FluentBuilder as _, relative,
};

use super::{Row, inside_edge, on_middle_click, row, section_button, section_head};
use crate::assets::Icon;
use crate::location::Location;
use crate::theme::{Colors, radius, size};
use crate::view::row::icon;
use crate::window::{DraggedTab, HoverOpens, MainWindow, TabChip, TabSide};

const LABEL: &str = "Open";

impl MainWindow {
    pub(super) fn open_section(&self, narrow: bool, cx: &Context<Self>) -> Option<Div> {
        if self.picker() {
            return None;
        }
        let colors = Colors::of(cx);
        let places = self.tab_places();
        let rows =
            self.tab_looks(cx)
                .into_iter()
                .zip(places)
                .enumerate()
                .map(|(index, (look, place))| {
                    let at = self.tab_drops_at(place.as_ref(), look.0.clone());
                    self.tab_row(index, look, narrow, cx)
                        .when_some(at, |row, at| self.drop_target(row, at, cx))
                        .map(|row| Self::opens_on_hover(row, HoverOpens::Tab(index), cx))
                });
        let new_tab = section_button("new-tab", Icon::Plus, &colors).on_click(
            cx.listener(|this, _: &ClickEvent, _, cx| this.new_tab(&Location::Start, cx)),
        );
        let list = div().flex().gap(size::PLACE_GAP).children(rows);
        if narrow {
            return Some(
                list.items_center()
                    .flex_none()
                    .child(new_tab)
                    .pr(size::GAP)
                    .border_r(size::LINE)
                    .border_color(colors.line),
            );
        }
        Some(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .min_h_0()
                .max_h(relative(size::OPEN_MAX_SHARE))
                .pb(size::GAP)
                .border_b(size::LINE)
                .border_color(colors.line)
                .child(section_head(
                    LABEL,
                    Some(new_tab.into_any_element()),
                    &colors,
                ))
                .child(
                    list.id("open-scroll")
                        .flex_col()
                        .min_h_0()
                        .overflow_y_scroll(),
                ),
        )
    }

    fn tab_row(
        &self,
        index: usize,
        (name, glyph): (SharedString, Icon),
        narrow: bool,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let colors = Colors::of(cx);
        let closable = self.tab_count() > 1;
        let on = index == self.current_tab();
        let group = format!("tab-{index}");
        let dragged = DraggedTab::new(cx.entity_id(), index, name.clone());
        row(
            Row {
                id: ("tab", index).into(),
                selector: group.clone(),
                glyph,
                name,
                number: None,
                here: on,
                pressed: on,
                lead: None,
            },
            narrow,
            &colors,
        )
        .group(group.clone())
        .when(!narrow, |row| row.pr(inside_edge(size::TAB_CLOSE_RIGHT)))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.switch_tab(index, cx)))
        .map(|row| on_middle_click(row, true, cx, move |this, cx| this.close_tab(index, cx)))
        .map(|row| self.reorders(row, dragged, narrow, cx))
        .when(closable, |row| {
            row.child(
                close_button(index, on, &group, &colors).on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        this.close_tab(index, cx);
                    },
                )),
            )
        })
    }
}

impl MainWindow {
    fn reorders(
        &self,
        row: Stateful<Div>,
        dragged: DraggedTab,
        narrow: bool,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let index = dragged.index();
        let colors = Colors::of(cx);
        let line = self
            .tab_landing_line(index, cx)
            .map(|side| landing_line(side, narrow, &colors));
        row.on_drag(dragged, |tab, _, _, cx| cx.new(|_| TabChip::new(tab)))
            .on_drag_move(
                cx.listener(move |this, moved: &DragMoveEvent<DraggedTab>, _, cx| {
                    let (at, bounds) = (moved.event.position, moved.bounds);
                    let after = if narrow {
                        at.x > bounds.center().x
                    } else {
                        at.y > bounds.center().y
                    };
                    let side = if after {
                        TabSide::After
                    } else {
                        TabSide::Before
                    };
                    let over = (index, bounds.contains(&at), side);
                    this.tab_dragged_over(&moved.drag(cx).clone(), over, cx);
                }),
            )
            .on_drop(cx.listener(|this, tab: &DraggedTab, _, cx| {
                this.tab_dropped(tab, cx);
            }))
            .when_some(line, |row, line| row.relative().child(line))
    }
}

fn landing_line(side: TabSide, narrow: bool, colors: &Colors) -> Div {
    let line = div()
        .debug_selector(|| "tab-landing".to_owned())
        .absolute()
        .rounded(radius::SMALL)
        .bg(colors.drop_edge);
    match (narrow, side) {
        (false, TabSide::Before) => line.top_0().left_0().right_0().h(size::DROP_EDGE),
        (false, TabSide::After) => line.bottom_0().left_0().right_0().h(size::DROP_EDGE),
        (true, TabSide::Before) => line.left_0().top_0().bottom_0().w(size::DROP_EDGE),
        (true, TabSide::After) => line.right_0().top_0().bottom_0().w(size::DROP_EDGE),
    }
}

fn close_button(index: usize, on: bool, group: &str, colors: &Colors) -> Stateful<Div> {
    let selector = format!("tab-close-{index}");
    div()
        .id(("tab-close", index))
        .debug_selector(move || selector)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(size::TAB_CLOSE)
        .rounded(radius::SMALL)
        .text_color(colors.text_dim)
        .when(!on, |button| {
            button
                .invisible()
                .group_hover(group.to_owned(), gpui::Styled::visible)
        })
        .hover(|button| button.bg(colors.press).text_color(colors.text))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(icon(Icon::Close, size::TAB_CLOSE_ICON, colors.text_dim))
}
