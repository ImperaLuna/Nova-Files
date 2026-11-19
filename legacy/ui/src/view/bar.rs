use gpui::{
    Action, App, ClickEvent, Context, Div, InteractiveElement as _, IntoElement,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, div,
    prelude::FluentBuilder as _,
};

use super::row::icon;
use nova_files_folders_api::ViewMode;

use crate::assets::Icon;
use crate::commands::{GoBack, GoForward, GoUp, ShowDetails, ShowIcons};
use crate::theme::{Colors, size};
use crate::window::MainWindow;

impl MainWindow {
    pub(super) fn top_bar(&self, colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
        let back = icon_button(
            "back",
            Icon::Back,
            self.back_target().is_some().then_some(&GoBack),
            colors,
        );
        let forward = icon_button(
            "forward",
            Icon::Forward,
            self.forward_target().is_some().then_some(&GoForward),
            colors,
        );
        let up = icon_button(
            "up",
            Icon::Up,
            self.parent().is_some().then_some(&GoUp),
            colors,
        );
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(size::GAP)
            .pr(size::GAP)
            .pb(size::GAP)
            .child(
                capsule(colors)
                    .px(size::CAPSULE_PAD)
                    .gap(size::CAPSULE_BUTTON_GAP)
                    .child(back)
                    .child(forward)
                    .child(up),
            )
            .child(self.path_bar(colors, cx))
            .child(
                self.search_box(
                    capsule(colors)
                        .debug_selector(|| "search".to_owned())
                        .flex_shrink()
                        .w(size::SEARCH_WIDTH)
                        .px(size::SEARCH_PAD)
                        .gap(size::SEARCH_GAP),
                    colors,
                    cx,
                ),
            )
            .children(self.parts.bars())
            .child(
                capsule(colors)
                    .debug_selector(|| "views".to_owned())
                    .px(size::CAPSULE_PAD)
                    .gap(size::CAPSULE_BUTTON_GAP)
                    .children(self.view_switch(colors, cx)),
            )
    }
}

use nova_files_shell::widgets::square;
pub(super) use nova_files_shell::widgets::{capsule, icon_button};

impl MainWindow {
    fn view_switch(&self, colors: &Colors, cx: &App) -> Vec<gpui::Stateful<Div>> {
        let showing = self.folder_view(cx).mode;
        [
            (
                "view-list",
                Icon::List,
                ViewMode::Details,
                &ShowDetails as &dyn Action,
            ),
            ("view-tiles", Icon::Tiles, ViewMode::Icons, &ShowIcons),
        ]
        .into_iter()
        .map(|(name, glyph, mode, action)| {
            let on = showing == mode;
            switch_button(name, glyph, on, colors).when(!on, |button| {
                let action = action.boxed_clone();
                button
                    .cursor_pointer()
                    .hover(|button| button.bg(colors.hover))
                    .on_click(move |_: &ClickEvent, window, cx| {
                        window.dispatch_action(action.boxed_clone(), cx);
                    })
            })
        })
        .collect()
    }
}

fn switch_button(
    name: &'static str,
    glyph: Icon,
    on: bool,
    colors: &Colors,
) -> gpui::Stateful<Div> {
    let tone = if on { colors.text } else { colors.text_faint };
    square(name)
        .when(on, |button| button.bg(colors.press))
        .child(icon(glyph, size::BUTTON_ICON, tone))
}
