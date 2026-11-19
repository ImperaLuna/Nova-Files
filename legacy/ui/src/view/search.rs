use std::path::PathBuf;

use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement as _, MouseButton,
    MouseDownEvent, ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::theme::{Colors, radius, size, text};
use crate::window::{FocusTo, MainWindow, short_folder};

impl MainWindow {
    pub(super) fn folder_link(
        &self,
        path: PathBuf,
        index: usize,
        cx: &Context<Self>,
    ) -> AnyElement {
        let colors = Colors::of(cx);
        let folder = path.parent().unwrap_or(&path);
        let label = short_folder(folder, &self.home);
        div()
            .id(("folder", index))
            .debug_selector(move || format!("folder-{index}"))
            .max_w_full()
            .h(size::FOLDER_LINK_HEIGHT)
            .flex()
            .items_center()
            .px(size::FOLDER_LINK_PAD)
            .ml(-size::FOLDER_LINK_PAD)
            .rounded(radius::SMALL)
            .text_size(text::DETAIL)
            .font_weight(text::DETAIL_WEIGHT)
            .text_color(colors.text_dim)
            .cursor_pointer()
            .hover(|link| link.bg(colors.press).text_color(colors.text))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, _| this.pressed_name = true),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                this.reveal(path.clone());
                cx.notify();
            }))
            .child(div().min_w_0().truncate().child(label))
            .into_any_element()
    }

    pub(super) fn settle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.search.focus_to.take() {
            Some(FocusTo::Results) => {
                window.focus(&self.focus, cx);
                if self.cursor().is_none() {
                    self.move_cursor(crate::selection::Move::Down, false, cx);
                }
            }
            Some(FocusTo::List) => window.focus(&self.keys_home(cx), cx),
            Some(FocusTo::Handle(handle)) => window.focus(&handle, cx),
            None => {}
        }
    }
}
