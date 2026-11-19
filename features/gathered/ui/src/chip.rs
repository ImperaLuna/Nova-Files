use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement as _, MouseButton, MouseDownEvent,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, div,
};
use nova_files_shell::Scene;
use nova_files_shell::theme::{Colors, radius, size};

use crate::{GatheredPage, GatheredPart};

const CHIP: &str = "gathered-folders";

pub fn chip(
    part: &mut GatheredPart,
    scene: &Scene,
    _: &mut Window,
    cx: &mut Context<GatheredPart>,
) -> Option<AnyElement> {
    let place = scene.on_page::<GatheredPage>()?.place;
    let colors = Colors::of(cx);
    let chip = div()
        .id(CHIP)
        .debug_selector(|| CHIP.to_owned())
        .flex()
        .flex_none()
        .items_center()
        .h(size::CHIP_HEIGHT)
        .px(size::CHIP_PAD - size::DROP_EDGE)
        .rounded(radius::ROW)
        .text_color(colors.text_soft)
        .cursor_pointer()
        .hover(|chip| chip.bg(colors.hover).text_color(colors.text))
        .active(|chip| chip.bg(colors.press))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |part, event: &MouseDownEvent, _, cx| {
                part.open_menu(place, event.position, cx);
            }),
        )
        .child(part.count(place));
    Some(chip.into_any_element())
}
