use gpui::{Context, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _, div};

use crate::assets::Icon;
use crate::commands::{GoBack, GoForward, GoUp};
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
            .children(self.search_part().map(|search| {
                let capsule = capsule(colors)
                    .debug_selector(|| "search".to_owned())
                    .flex_shrink()
                    .w(size::SEARCH_WIDTH)
                    .px(size::SEARCH_PAD)
                    .gap(size::SEARCH_GAP);
                search.read(cx).search_box(capsule, colors, &search, cx)
            }))
            .children(self.parts.bars())
    }
}

pub(super) use nova_files_shell::widgets::{capsule, icon_button};
