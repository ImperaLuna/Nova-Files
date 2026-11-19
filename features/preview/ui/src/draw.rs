use std::ffi::OsStr;
use std::sync::Arc;

use gpui::{
    Action, AnyElement, ClickEvent, Context, Div, InteractiveElement as _, IntoElement,
    MouseButton, MouseDownEvent, ParentElement as _, Pixels, RenderImage, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, img, prelude::FluentBuilder as _, px,
};
use nova_files_api::{Entry, EntryKind, Listing};
use nova_files_preview_api::{Preview, PreviewContent};
use nova_files_shell::actions::{PreviewNext, PreviewPrevious};
use nova_files_shell::assets::Icon;
use nova_files_shell::format;
use nova_files_shell::kind::{FileKind, built_in, icon_kind};
use nova_files_shell::theme::{Colors, preview, radius, size, text};
use nova_files_shell::widgets::{icon, kind_icon};

use crate::PreviewPart;
use crate::looks::{QuickLook, Shown};

const SEPARATOR: &str = " · ";
const FILE_KIND: &str = "File";
const FOLDER_KIND: &str = "Folder";
const ITEM: &str = "item";
const ITEMS: &str = "items";
const MORE: &str = "more";
const OPEN_LABEL: &str = "Open";
const BOTH_SIDES: f32 = 2.0;

pub fn room(width: Pixels, height: Pixels) -> gpui::Size<Pixels> {
    let sides = preview::MARGIN * BOTH_SIDES + (preview::NAV + preview::GAP) * BOTH_SIDES;
    let above_and_below = preview::MARGIN * BOTH_SIDES + preview::CAPTION_HEIGHT + preview::GAP;
    gpui::size(
        (width - sides).max(px(0.0)),
        (height - above_and_below).max(px(0.0)),
    )
}

pub fn layer(
    (open, listing, entry, index): (&QuickLook, &Listing, &Entry, usize),
    (room, scale_factor): (gpui::Size<Pixels>, f32),
    cx: &Context<PreviewPart>,
) -> AnyElement {
    let colors = Colors::of(cx);
    let current = open.current().and_then(|shown| shown.preview.as_deref());
    let art = stage_art(open, listing, entry, (room, scale_factor), &colors);
    let (previous, next) = steps(index, listing.entries.len());
    div()
        .id("preview")
        .debug_selector(|| "preview".to_owned())
        .absolute()
        .inset_0()
        .occlude()
        .bg(colors.scrim)
        .flex()
        .items_center()
        .justify_center()
        .gap(preview::GAP)
        .p(preview::MARGIN)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| this.dismiss(cx)),
        )
        .child(kept_open(nav_button(
            "preview-previous",
            Icon::Back,
            previous.then_some(&PreviewPrevious as &dyn Action),
            &colors,
        )))
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(preview::GAP)
                .min_w_0()
                .child(kept_open(art).debug_selector(|| "preview-art".to_owned()))
                .child(caption(entry, current, &colors, cx)),
        )
        .child(kept_open(nav_button(
            "preview-next",
            Icon::Forward,
            next.then_some(&PreviewNext as &dyn Action),
            &colors,
        )))
        .into_any_element()
}

pub fn stage_art(
    open: &QuickLook,
    listing: &Listing,
    entry: &Entry,
    (room, scale_factor): (gpui::Size<Pixels>, f32),
    colors: &Colors,
) -> AnyElement {
    open.shown.as_ref().map_or_else(
        || big_icon(&entry.name, entry.kind, colors),
        |shown| {
            art(
                shown,
                shown_entry(shown, listing, entry),
                room,
                scale_factor,
                colors,
            )
        },
    )
}

fn nav_button(
    name: &'static str,
    glyph: Icon,
    action: Option<&dyn Action>,
    colors: &Colors,
) -> impl IntoElement {
    let tone = if action.is_some() {
        colors.text_soft
    } else {
        colors.text_faint
    };
    div()
        .id(name)
        .debug_selector(|| name.to_owned())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(size::ICON_BUTTON)
        .rounded(radius::ROW)
        .child(icon(glyph, size::BUTTON_ICON, tone))
        .when_some(action.map(Action::boxed_clone), |button, action| {
            button
                .cursor_pointer()
                .hover(|button| button.bg(colors.hover))
                .active(|button| button.bg(colors.press))
                .on_click(move |_: &ClickEvent, window, cx| {
                    window.dispatch_action(action.boxed_clone(), cx);
                })
        })
}

fn shown_entry<'a>(shown: &Shown, listing: &'a Listing, entry: &'a Entry) -> &'a Entry {
    usize::try_from(shown.entry.0)
        .ok()
        .and_then(|index| listing.entries.get(index))
        .unwrap_or(entry)
}

fn kept_open(element: impl IntoElement) -> Div {
    div()
        .flex_none()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(element)
}

fn art(
    shown: &Shown,
    entry: &Entry,
    room: gpui::Size<Pixels>,
    scale_factor: f32,
    colors: &Colors,
) -> AnyElement {
    let Some(preview) = shown.preview.as_deref() else {
        return big_icon(&entry.name, entry.kind, colors);
    };
    let (name, kind) = (&entry.name, entry.kind);
    match (&preview.content, &shown.picture) {
        (PreviewContent::Picture(_), Some(picture)) => {
            picture_art(picture, room, scale_factor, colors).into_any_element()
        }
        (PreviewContent::Text { text, .. }, _) => text_art(text, room, colors).into_any_element(),
        (PreviewContent::Folder { first, count }, _) => folder_art(first, *count, colors),
        _ => big_icon(name, kind, colors),
    }
}

fn picture_art(
    picture: &Arc<RenderImage>,
    room: gpui::Size<Pixels>,
    scale_factor: f32,
    colors: &Colors,
) -> impl IntoElement {
    let size = picture.size(0);
    let logical = |device: i32| logical_side(device, scale_factor);
    let drawn = fitted(
        gpui::size(logical(size.width.0), logical(size.height.0)),
        room,
    );
    img(Arc::clone(picture))
        .debug_selector(|| "preview-picture".to_owned())
        .w(drawn.width)
        .h(drawn.height)
        .rounded(radius::CARD)
        .border(size::LINE)
        .border_color(colors.line)
}

fn logical_side(device: i32, scale_factor: f32) -> Pixels {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a picture's side is a few thousand pixels at most"
    )]
    let device = device as f32;
    px(device / scale_factor)
}

const fn steps(index: usize, len: usize) -> (bool, bool) {
    (index > 0, index + 1 < len)
}

fn lines_fitting(height: Pixels) -> usize {
    let lines = (f32::from(height - preview::TEXT_PAD_Y * BOTH_SIDES)
        / f32::from(preview::TEXT_LINE))
    .floor();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a count of lines that fit a window, never negative"
    )]
    let lines = lines.max(1.0) as usize;
    lines
}

fn fitted(size: gpui::Size<Pixels>, room: gpui::Size<Pixels>) -> gpui::Size<Pixels> {
    let scale = fitting_scale(size, room);
    gpui::size(size.width * scale, size.height * scale)
}

fn fitting_scale(size: gpui::Size<Pixels>, room: gpui::Size<Pixels>) -> f32 {
    let ratio = |room: Pixels, side: Pixels| {
        if side > px(0.0) {
            f32::from(room) / f32::from(side)
        } else {
            1.0
        }
    };
    ratio(room.width, size.width)
        .min(ratio(room.height, size.height))
        .clamp(0.0, 1.0)
}

fn text_art(text_content: &str, room: gpui::Size<Pixels>, colors: &Colors) -> impl IntoElement {
    let lines = lines_fitting(room.height);
    card(preview::TEXT_WIDTH, colors)
        .debug_selector(|| "preview-text".to_owned())
        .max_h(room.height)
        .px(preview::TEXT_PAD_X)
        .py(preview::TEXT_PAD_Y)
        .bg(colors.window)
        .line_height(preview::TEXT_LINE)
        .text_color(colors.text_soft)
        .whitespace_nowrap()
        .children(
            text_content
                .lines()
                .take(lines)
                .map(|line| div().truncate().child(SharedString::from(line.to_owned()))),
        )
}

fn folder_art(first: &[Entry], count: usize, colors: &Colors) -> AnyElement {
    if first.is_empty() {
        return big_icon(OsStr::new(""), EntryKind::Folder, colors);
    }
    let more = count.saturating_sub(first.len());
    card(preview::LIST_WIDTH, colors)
        .debug_selector(|| "preview-folder".to_owned())
        .px(preview::LIST_PAD_X)
        .py(preview::LIST_PAD_Y)
        .bg(colors.raised)
        .children(first.iter().map(|entry| {
            div()
                .flex()
                .items_center()
                .gap(preview::LIST_GAP)
                .h(preview::LIST_ROW)
                .min_w_0()
                .text_color(colors.text_soft)
                .child(kind_icon(
                    icon_kind(&entry.name, entry.kind),
                    size::ROW_ICON,
                    colors,
                ))
                .child(div().min_w_0().truncate().child(SharedString::from(
                    entry.name.to_string_lossy().into_owned(),
                )))
        }))
        .when(more > 0, |list| {
            list.child(
                div()
                    .debug_selector(|| "preview-more".to_owned())
                    .h(preview::LIST_ROW)
                    .flex()
                    .items_center()
                    .text_size(text::DETAIL)
                    .text_color(colors.text_dim)
                    .child(format!("{more} {MORE}")),
            )
        })
        .into_any_element()
}

fn card(width: Pixels, colors: &Colors) -> Div {
    div()
        .w(width)
        .max_w_full()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(radius::CARD)
        .border(size::LINE)
        .border_color(colors.line)
}

fn big_icon(name: &OsStr, kind: EntryKind, colors: &Colors) -> AnyElement {
    let (glyph, tint) = match kind {
        EntryKind::Folder => (Icon::Folder, colors.folder),
        EntryKind::File => {
            let (glyph, _) = built_in(icon_kind(name, kind), colors);
            (glyph, FileKind::of(name).tag(colors.tags))
        }
    };
    card(preview::CARD_WIDTH, colors)
        .debug_selector(|| "preview-icon".to_owned())
        .h(preview::CARD_HEIGHT)
        .bg(colors.window)
        .items_center()
        .justify_center()
        .child(icon(glyph, preview::BIG_ICON, tint))
        .into_any_element()
}

fn caption(
    entry: &Entry,
    current: Option<&Preview>,
    colors: &Colors,
    cx: &Context<PreviewPart>,
) -> impl IntoElement {
    let name = SharedString::from(entry.name.to_string_lossy().into_owned());
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(preview::CAPTION_GAP)
        .h(preview::CAPTION_HEIGHT)
        .max_w_full()
        .min_w_0()
        .pl(preview::CAPTION_PAD_START)
        .pr(preview::CAPTION_PAD_END)
        .rounded(radius::CARD)
        .bg(colors.lifted)
        .border(size::LINE)
        .border_color(colors.line)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .flex()
                .min_w_0()
                .mr(preview::CAPTION_TEXT_GAP)
                .whitespace_nowrap()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(colors.text)
                        .font_weight(text::STRONG_WEIGHT)
                        .child(name),
                )
                .children(current.map(|preview| {
                    div()
                        .flex_none()
                        .text_color(colors.text_soft)
                        .child(format!("{SEPARATOR}{}", meta(preview)))
                })),
        )
        .child(
            div()
                .id("preview-open")
                .debug_selector(|| "preview-open".to_owned())
                .flex()
                .flex_none()
                .items_center()
                .h(preview::BUTTON_HEIGHT)
                .px(preview::BUTTON_PAD)
                .rounded(radius::ROW)
                .text_color(colors.accept)
                .font_weight(text::STRONG_WEIGHT)
                .cursor_pointer()
                .hover(|button| button.bg(colors.hover))
                .active(|button| button.bg(colors.press))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.open_previewed(cx);
                }))
                .child(OPEN_LABEL),
        )
        .child(
            div()
                .id("preview-close")
                .debug_selector(|| "preview-close".to_owned())
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(size::ICON_BUTTON)
                .rounded(radius::ROW)
                .cursor_pointer()
                .hover(|button| button.bg(colors.hover))
                .active(|button| button.bg(colors.press))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.dismiss(cx);
                }))
                .child(icon(Icon::Close, size::BUTTON_ICON, colors.text_soft)),
        )
}

pub fn meta(preview: &Preview) -> String {
    if let PreviewContent::Folder { count, .. } = &preview.content {
        let unit = if *count == 1 { ITEM } else { ITEMS };
        return format!("{FOLDER_KIND}{SEPARATOR}{count} {unit}");
    }
    let kind = preview.kind_name.as_deref().unwrap_or(FILE_KIND);
    format!("{kind}{SEPARATOR}{}", format::size(preview.bytes))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn a_picture_shrinks_to_fit_but_never_grows() {
        let fits = fitting_scale(
            gpui::size(px(100.0), px(50.0)),
            gpui::size(px(400.0), px(400.0)),
        );
        assert!((fits - 1.0).abs() < f32::EPSILON);
        let wide = fitting_scale(
            gpui::size(px(200.0), px(50.0)),
            gpui::size(px(100.0), px(400.0)),
        );
        assert!((wide - 0.5).abs() < f32::EPSILON);
        let tall = fitting_scale(
            gpui::size(px(100.0), px(400.0)),
            gpui::size(px(400.0), px(100.0)),
        );
        assert!((tall - 0.25).abs() < f32::EPSILON);
        let none = fitting_scale(
            gpui::size(px(100.0), px(50.0)),
            gpui::size(px(0.0), px(0.0)),
        );
        assert!(none.abs() < f32::EPSILON);
        let empty = fitting_scale(gpui::size(px(0.0), px(0.0)), gpui::size(px(0.0), px(0.0)));
        assert!((empty - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_picture_is_drawn_at_its_fitted_size() {
        let drawn = fitted(
            gpui::size(px(200.0), px(100.0)),
            gpui::size(px(100.0), px(400.0)),
        );
        assert_eq!(drawn, gpui::size(px(100.0), px(50.0)));
    }

    #[test]
    fn device_pixels_are_divided_by_the_scale() {
        assert_eq!(logical_side(600, 2.0), px(300.0));
        assert_eq!(logical_side(7, 1.0), px(7.0));
    }

    #[test]
    fn previous_and_next_need_an_entry_that_way() {
        assert_eq!(steps(0, 3), (false, true));
        assert_eq!(steps(1, 3), (true, true));
        assert_eq!(steps(2, 3), (true, false));
        assert_eq!(steps(0, 1), (false, false));
    }

    #[test]
    fn as_many_lines_fit_as_the_room_holds() {
        let pads = preview::TEXT_PAD_Y * BOTH_SIDES;
        let line = preview::TEXT_LINE;
        assert_eq!(lines_fitting(pads + line * 10.0), 10);
        assert_eq!(lines_fitting(pads + line * 10.5), 10);
        assert_eq!(lines_fitting(pads + line * 3.0), 3);
        assert_eq!(lines_fitting(px(0.0)), 1);
    }

    fn preview(content: PreviewContent, kind_name: Option<&str>, bytes: u64) -> Preview {
        Preview {
            path: PathBuf::from("/a/b"),
            kind_name: kind_name.map(str::to_owned),
            bytes,
            content,
        }
    }

    #[test]
    fn a_file_shows_its_kind_and_size() {
        let shown = preview(PreviewContent::Icon, Some("PNG image"), 1_800_000);
        assert_eq!(meta(&shown), "PNG image · 1.8 MB");
        let unknown = preview(PreviewContent::Icon, None, 512);
        assert_eq!(meta(&unknown), "File · 512 B");
    }

    #[test]
    fn a_folder_shows_how_many_entries_it_holds() {
        let folder = |count| {
            preview(
                PreviewContent::Folder {
                    first: Vec::new(),
                    count,
                },
                None,
                0,
            )
        };
        assert_eq!(meta(&folder(1)), "Folder · 1 item");
        assert_eq!(meta(&folder(12)), "Folder · 12 items");
    }

    #[test]
    fn the_room_leaves_the_margins_buttons_and_caption() {
        let room = room(px(1100.0), px(720.0));
        assert_eq!(room.width, px(1100.0 - 32.0 - 104.0));
        assert_eq!(room.height, px(720.0 - 32.0 - 46.0 - 12.0));
        assert_eq!(
            super::room(px(10.0), px(10.0)),
            gpui::size(px(0.0), px(0.0))
        );
    }
}
