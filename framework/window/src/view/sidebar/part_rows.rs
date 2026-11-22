use std::f32::consts::FRAC_PI_2;

use gpui::{
    AnyElement, App, ClickEvent, Context, Div, InteractiveElement as _, IntoElement, MouseButton,
    MouseDownEvent, ParentElement as _, Pixels, Rgba, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Transformation, Window, div,
    prelude::FluentBuilder as _, radians, svg,
};
use nova_files_shell::{Expander, Gap, Look, RowItem, SectionButton, SidebarAt, SidebarRow};

use super::{Row, icon, on_middle_click, row, section_button};
use crate::assets::Icon;
use crate::theme::{Colors, radius, size};
use crate::window::{Aim, DraggedRow, HoverOpens, MainWindow};

impl MainWindow {
    pub(super) fn part_rows(
        &self,
        at: SidebarAt,
        narrow: bool,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> Vec<(u8, Stateful<Div>)> {
        let tree = at == SidebarAt::Tree && !narrow;
        self.parts
            .sidebar(at, &self.scene(cx), cx)
            .into_iter()
            .map(|place| {
                let rank = place.rank;
                let row = if tree {
                    self.tree_row(place, colors, cx)
                } else {
                    self.part_row(place, narrow, colors, cx)
                };
                (rank, row)
            })
            .collect()
    }

    pub(super) fn part_buttons(
        &self,
        at: SidebarAt,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        if self.picker() {
            return Vec::new();
        }
        self.parts
            .section_buttons(at, &self.scene(cx), cx)
            .into_iter()
            .map(|(id, button)| {
                let shown = section_button(id, button.icon(), colors);
                match button {
                    SectionButton::Runs(item) => shown.on_click(run_on_click(item, cx)),
                    SectionButton::Opens { groups, .. } => {
                        shown.on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            this.open_part_menu(event.position(), groups.clone(), cx);
                        }))
                    }
                }
                .into_any_element()
            })
            .collect()
    }

    fn part_row(
        &self,
        place: SidebarRow,
        narrow: bool,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let wide = !narrow;
        let lead = place
            .expander
            .clone()
            .filter(|_| wide)
            .map(|expander| expander_button(&place.id, expander, colors, cx));
        let tail = place
            .button
            .clone()
            .filter(|_| wide)
            .map(|button| row_button(&place.id, button, colors, cx));
        let shown = row(
            Row {
                id: place.id.clone().into(),
                selector: place.id.to_string(),
                glyph: place.icon,
                name: place.name.clone(),
                number: place
                    .number
                    .clone()
                    .filter(|_| wide)
                    .map(|number| (number, number_tint(place.number_look, colors))),
                here: place.here,
                pressed: false,
                lead,
            },
            narrow,
            colors,
        )
        .when(wide && place.depth > 0, |row| {
            row.pl(size::TREE_INDENT * f32::from(place.depth))
        })
        .children(tail);
        let gaps = [(place.above.clone(), false), (place.below.clone(), true)];
        let id = place.id.clone();
        let shown = self.part_row_acts(shown, place, cx);
        if narrow || !cx.has_active_drag() {
            return shown;
        }
        shown.relative().children(
            gaps.into_iter()
                .filter_map(|(gap, below)| gap.map(|gap| self.gap_line(&id, gap, below, cx))),
        )
    }

    fn gap_line(&self, row: &str, gap: Gap, below: bool, cx: &Context<Self>) -> AnyElement {
        let selector = format!("{row}-{}", if below { "below" } else { "above" });
        let lit = Colors::of(cx).drop_edge;
        let line = div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector)
            .absolute()
            .left_0()
            .right_0()
            .h(size::PIN_LINE)
            .rounded(radius::SMALL)
            .occlude()
            .map(|line| {
                if below {
                    line.bottom(size::PIN_LINE_OVERHANG)
                } else {
                    line.top(size::PIN_LINE_OVERHANG)
                }
            })
            .drag_over::<DraggedRow>(move |style, _, _, _| style.bg(lit));
        self.takes_drops(line, Aim::gap(gap), cx).into_any_element()
    }

    pub(super) fn part_row_acts(
        &self,
        shown: Stateful<Div>,
        place: SidebarRow,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let main = !self.picker();
        let menu_row = place.id.clone();
        let at = place.opens.clone();
        let drops = at
            .as_ref()
            .and_then(|at| self.tab_drops_at(Some(at), place.name.clone()));
        let clicked = Clicked {
            opens: place.opens.clone(),
            open: place.open.clone(),
        };
        let middle = Clicked {
            opens: place.opens,
            open: place.new_tab,
        };
        let shown = shown
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                clicked.follow(this, false, cx);
            }))
            .map(|row| {
                on_middle_click(row, main, cx, move |this, cx| middle.follow(this, true, cx))
            })
            .when_some(at.filter(|at| at.folder().is_some()), |row, at| {
                Self::opens_on_hover(row, HoverOpens::Place(at), cx)
            })
            .when(main, |row| {
                row.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        this.open_row_menu(&menu_row, event.position, cx);
                    }),
                )
            });
        match drops {
            Some(at) => self.drop_target(shown, at, cx),
            None => shown,
        }
    }
}

pub(super) fn expander_button(
    id: &str,
    expander: Expander,
    colors: &Colors,
    cx: &Context<MainWindow>,
) -> AnyElement {
    let angle = if expander.open { FRAC_PI_2 } else { 0.0 };
    small_button(format!("{id}-expander"), size::TREE_TOGGLE, colors)
        .on_click(run_on_click(expander.toggle, cx))
        .child(
            svg()
                .path(Icon::Chevron.path())
                .size(size::TREE_TOGGLE_ICON)
                .text_color(colors.text_faint)
                .with_transformation(Transformation::rotate(radians(angle))),
        )
        .into_any_element()
}

fn row_button(id: &str, button: RowItem, colors: &Colors, cx: &Context<MainWindow>) -> AnyElement {
    small_button(format!("{id}-button"), size::SECTION_BUTTON, colors)
        .child(icon(
            button.icon,
            size::SECTION_BUTTON_ICON,
            colors.text_dim,
        ))
        .on_click(run_on_click(button, cx))
        .into_any_element()
}

fn small_button(selector: String, side: Pixels, colors: &Colors) -> Stateful<Div> {
    div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(side)
        .rounded(radius::SMALL)
        .cursor_pointer()
        .hover(|button| button.bg(colors.press))
}

struct Clicked {
    opens: Option<nova_files_shell::Location>,
    open: Option<RowItem>,
}

impl Clicked {
    fn follow(&self, window: &mut MainWindow, new_tab: bool, cx: &mut Context<MainWindow>) {
        match (&self.open, &self.opens) {
            (Some(open), _) => run_later(open, cx),
            (None, Some(opens)) if new_tab => window.new_tab(opens, cx),
            (None, Some(opens)) => window.go_to_place(opens, cx),
            (None, None) => {}
        }
    }
}

fn run_later(item: &RowItem, cx: &mut Context<MainWindow>) {
    let run = item.run.clone();
    cx.defer(move |cx| run(cx));
}

fn run_on_click(
    item: RowItem,
    cx: &Context<MainWindow>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    cx.listener(move |_, _: &ClickEvent, _, cx| {
        cx.stop_propagation();
        let run = item.run.clone();
        cx.defer(move |cx| run(cx));
    })
}

const fn number_tint(look: Look, colors: &Colors) -> Rgba {
    match look {
        Look::Plain => colors.text_dim,
        Look::On => colors.here,
        Look::Danger => colors.full,
    }
}
