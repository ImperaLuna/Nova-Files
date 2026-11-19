use gpui::{
    Action, AnyElement, App, ClickEvent, Div, InteractiveElement as _, IntoElement,
    ParentElement as _, Stateful, StatefulInteractiveElement as _, Styled as _,
    prelude::FluentBuilder as _,
};
use nova_files_folders_api::ViewMode;
use nova_files_shell::actions::{ShowDetails, ShowIcons};
use nova_files_shell::assets::Icon;
use nova_files_shell::theme::{Colors, size};
use nova_files_shell::widgets::{capsule, icon, square};

use crate::FoldersPart;

impl FoldersPart {
    pub(crate) fn view_switch(&self, cx: &App) -> AnyElement {
        let colors = Colors::of(cx);
        let showing = self.shown(cx).mode;
        let buttons = [
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
            switch_button(name, glyph, on, &colors).when(!on, |button| {
                let action = action.boxed_clone();
                button
                    .cursor_pointer()
                    .hover(|button| button.bg(colors.hover))
                    .on_click(move |_: &ClickEvent, window, cx| {
                        window.dispatch_action(action.boxed_clone(), cx);
                    })
            })
        });
        capsule(&colors)
            .debug_selector(|| "views".to_owned())
            .px(size::CAPSULE_PAD)
            .gap(size::CAPSULE_BUTTON_GAP)
            .children(buttons)
            .into_any_element()
    }
}

fn switch_button(name: &'static str, glyph: Icon, on: bool, colors: &Colors) -> Stateful<Div> {
    let tone = if on { colors.text } else { colors.text_faint };
    square(name)
        .when(on, |button| button.bg(colors.press))
        .child(icon(glyph, size::BUTTON_ICON, tone))
}
