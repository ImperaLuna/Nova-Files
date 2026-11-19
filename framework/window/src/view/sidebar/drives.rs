use gpui::{
    Context, Div, InteractiveElement as _, ParentElement as _, Stateful,
    StatefulInteractiveElement as _, Styled as _, div,
};

use nova_files_shell::SidebarAt;

use super::section_head;
use crate::theme::{Colors, size};
use crate::window::MainWindow;

const LABEL: &str = "Drives";

impl MainWindow {
    pub(super) fn drives_section(&self, narrow: bool, cx: &Context<Self>) -> Div {
        let colors = Colors::of(cx);
        let rows = self.part_rows(SidebarAt::Drives, narrow, &colors, cx);
        let mut ranked: Vec<(u8, Stateful<Div>)> = rows;
        ranked.sort_by_key(|(rank, _)| *rank);
        let list = div()
            .flex()
            .gap(size::PLACE_GAP)
            .children(ranked.into_iter().map(|(_, row)| row));
        if narrow {
            return list;
        }
        div()
            .flex()
            .flex_col()
            .min_h_0()
            .pb(size::GAP)
            .border_t(size::LINE)
            .border_color(colors.line)
            .child(section_head(
                LABEL,
                self.part_buttons(SidebarAt::Drives, &colors, cx),
                &colors,
            ))
            .child(
                list.id("drives-scroll")
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll(),
            )
    }
}
