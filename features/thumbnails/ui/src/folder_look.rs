use std::sync::Arc;

use gpui::{
    AnyElement, Div, InteractiveElement as _, IntoElement as _, ParentElement as _, Pixels,
    RenderImage, Styled as _, div, img, px,
};
use nova_files_api::IconKind;
use nova_files_shell::kind::glyph_of;
use nova_files_shell::theme::{Colors, radius};
use nova_files_shell::widgets::{icon, icon_slot, kind_icon};

use crate::peeks::PeekItem;

pub const PEEK_FROM: Pixels = px(96.0);
pub const MARK_FROM: Pixels = px(48.0);
pub const PEEK_SHARE: f32 = 0.44;

const EDGE: f32 = 0.04;
const WIDTH: f32 = 0.92;
const TAB_TOP: f32 = 0.19;
const TAB_WIDTH: f32 = 0.38;
const BACK_TOP: f32 = 0.26;
const BACK_HEIGHT: f32 = 0.62;
const FRONT_TOP: f32 = 0.42;
const FRONT_HEIGHT: f32 = 0.46;
const CORNER: f32 = 0.1;
const BACK_SHADE: f32 = 0.5;
const PEEK_FLOOR: f32 = 0.6;
const PEEK_CENTRES: [&[f32]; 4] = [&[], &[0.5], &[0.38, 0.62], &[0.3, 0.5, 0.7]];
const MARK_SHARE: f32 = 0.3;
const MARK_MIDDLE: f32 = 0.6;
const MARK_SHADE: f32 = 0.45;
const HALF: f32 = 0.5;

pub fn folder(
    entry: u32,
    items: &[&PeekItem],
    side: Pixels,
    colors: &Colors,
) -> Option<AnyElement> {
    if side >= PEEK_FROM {
        let look =
            peeking(entry, items, side, colors).debug_selector(move || format!("peek-{entry}"));
        Some(look.into_any_element())
    } else if side >= MARK_FROM {
        let look = marked(items.first().map(|item| item.kind), side, colors)
            .debug_selector(move || format!("folder-mark-{entry}"));
        Some(look.into_any_element())
    } else {
        None
    }
}

fn peeking(entry: u32, items: &[&PeekItem], side: Pixels, colors: &Colors) -> Div {
    let at = |share: f32| side * share;
    let plate = |top: f32, height: f32| {
        div()
            .absolute()
            .left(at(EDGE))
            .top(at(top))
            .w(at(WIDTH))
            .h(at(height))
            .rounded(at(CORNER))
            .bg(colors.folder)
    };
    let tab = div()
        .absolute()
        .left(at(EDGE))
        .top(at(TAB_TOP))
        .w(at(TAB_WIDTH))
        .h(at(BACK_TOP - TAB_TOP))
        .rounded_t(at(CORNER))
        .bg(colors.folder)
        .opacity(BACK_SHADE);
    let centres = PEEK_CENTRES.get(items.len()).copied().unwrap_or_default();
    let box_side = at(PEEK_SHARE);
    let peeks = items
        .iter()
        .zip(centres)
        .zip(0..)
        .map(|((item, centre), place)| {
            div()
                .debug_selector(move || format!("peek-{entry}-{place}"))
                .absolute()
                .left(at(*centre) - box_side * HALF)
                .top(at(PEEK_FLOOR) - box_side)
                .child(peeked(item, box_side, colors))
        });
    icon_slot(side)
        .child(tab)
        .child(
            plate(BACK_TOP, BACK_HEIGHT)
                .rounded_tl(Pixels::ZERO)
                .opacity(BACK_SHADE),
        )
        .children(peeks)
        .child(plate(FRONT_TOP, FRONT_HEIGHT))
}

fn peeked(item: &PeekItem, side: Pixels, colors: &Colors) -> AnyElement {
    item.image.as_ref().map_or_else(
        || kind_icon(item.kind, side, colors).into_any_element(),
        |image| picture(image, side).into_any_element(),
    )
}

pub fn picture(image: &Arc<RenderImage>, side: Pixels) -> Div {
    let drawn = image.size(0);
    let longest = drawn.width.max(drawn.height).0.max(1);
    let share = |along: i32| {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a thumbnail side is a few hundred pixels"
        )]
        let along = along as f32 / longest as f32;
        side * along
    };
    icon_slot(side).items_end().child(
        img(Arc::clone(image))
            .w(share(drawn.width.0))
            .h(share(drawn.height.0))
            .rounded(radius::SMALL),
    )
}

fn marked(kind: Option<IconKind>, side: Pixels, colors: &Colors) -> Div {
    let mark = kind.map(|kind| {
        let mark_side = side * MARK_SHARE;
        div()
            .absolute()
            .left((side - mark_side) * HALF)
            .top(side * MARK_MIDDLE - mark_side * HALF)
            .opacity(MARK_SHADE)
            .child(icon(glyph_of(kind), mark_side, colors.window))
    });
    kind_icon(IconKind::Folder, side, colors).children(mark)
}
