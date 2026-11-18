//! Draws the quick look over the whole window, as on the design page: a
//! scrim, what is shown large in the middle with its caption under it, and
//! round Previous and Next buttons either side.
//!
//! What is shown: a picture at the size the core decoded it (which fits
//! the room, see `window::preview`), scaled down if the window has since
//! shrunk, a text file's first lines as text, a
//! folder's first entries with how many more it holds, and anything else
//! as its large icon. The caption names the entry, its kind and its size
//! (a folder's count of entries), with Open and Close buttons. A click on
//! the scrim closes the quick look; a click on what is shown or the
//! caption doesn't. While nothing has come yet for the entry, the caption
//! shows its name and what was shown before stays.
//!
//! Selectors for tests: `preview`, `preview-art`, `preview-open`,
//! `preview-close`, `preview-previous` and `preview-next`; what is shown
//! is `preview-picture`, `preview-text`, `preview-folder` (with
//! `preview-more` under it when the folder holds more) or `preview-icon`.

use std::ffi::OsStr;
use std::sync::Arc;

use gpui::{
    AnyElement, ClickEvent, Context, Div, InteractiveElement as _, IntoElement, MouseButton,
    MouseDownEvent, ParentElement as _, Pixels, RenderImage, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, img, prelude::FluentBuilder as _, px,
};
use nova_files_api::{Entry, EntryKind, Preview, PreviewContent};

use super::bar::icon_button;
use super::row::{RowIcon, built_in, entry_icon, icon};
use crate::assets::Icon;
use crate::columns;
use crate::commands::{PreviewNext, PreviewPrevious};
use crate::kind::{FileKind, icon_kind};
use crate::theme::{Colors, preview, radius, size, text};
use crate::window::{MainWindow, QuickLook, Shown};

/// Between the parts of the caption's text.
const SEPARATOR: &str = " · ";
/// A file's kind when the MIME database has no name for it.
const FILE_KIND: &str = "File";
const FOLDER_KIND: &str = "Folder";
const ITEM: &str = "item";
const ITEMS: &str = "items";
const MORE: &str = "more";
const OPEN_LABEL: &str = "Open";
/// A margin or padding counts once on each side.
const BOTH_SIDES: f32 = 2.0;

/// The room the quick look leaves what it shows, in a window `width` by
/// `height`.
pub fn room(width: Pixels, height: Pixels) -> gpui::Size<Pixels> {
    let sides = preview::MARGIN * BOTH_SIDES + (preview::NAV + preview::GAP) * BOTH_SIDES;
    let above_and_below = preview::MARGIN * BOTH_SIDES + preview::CAPTION_HEIGHT + preview::GAP;
    gpui::size(
        (width - sides).max(px(0.0)),
        (height - above_and_below).max(px(0.0)),
    )
}

impl MainWindow {
    /// The quick look, while it is open.
    pub(super) fn preview_layer(
        &self,
        room: gpui::Size<Pixels>,
        scale_factor: f32,
        cx: &Context<Self>,
    ) -> Option<impl IntoElement> {
        let open = self.quick_look().filter(|_| self.previewing())?;
        let listing = self
            .listing
            .as_ref()
            .filter(|listing| listing.id == open.listing)?;
        let index = usize::try_from(open.entry.0).ok()?;
        let entry = listing.entries.get(index)?;
        let colors = Colors::of(cx);
        // The preview itself, once it is this entry's.
        let current = open.current().and_then(|shown| shown.preview.as_deref());
        let art = stage_art(open, listing, entry, (room, scale_factor), &colors);
        let (previous, next) = steps(index, listing.entries.len());
        Some(
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
                    cx.listener(|this, _: &MouseDownEvent, window, cx| {
                        this.dismiss_preview(window, cx);
                    }),
                )
                .child(kept_open(icon_button(
                    "preview-previous",
                    Icon::Back,
                    previous.then_some(&PreviewPrevious as &dyn gpui::Action),
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
                .child(kept_open(icon_button(
                    "preview-next",
                    Icon::Forward,
                    next.then_some(&PreviewNext as &dyn gpui::Action),
                    &colors,
                ))),
        )
    }
}

/// What the quick look `open` on `entry` of `listing` draws large in
/// `room` at a scale factor: what the core sent, or the entry's icon
/// until it does.
pub(super) fn stage_art(
    open: &QuickLook,
    listing: &nova_files_api::Listing,
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

/// `element` in a box a press doesn't pass through, so a click on it
/// doesn't reach the scrim and close the quick look.
fn kept_open(element: impl IntoElement) -> Div {
    div()
        .flex_none()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(element)
}

/// The entry `shown` is of: the one on screen, or the one shown before
/// while this one loads.
fn shown_entry<'a>(
    shown: &Shown,
    listing: &'a nova_files_api::Listing,
    entry: &'a Entry,
) -> &'a Entry {
    usize::try_from(shown.entry.0)
        .ok()
        .and_then(|index| listing.entries.get(index))
        .unwrap_or(entry)
}

/// What `shown` draws large: its picture, text, folder list or icon.
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

/// A picture at the size it was decoded, which fit the room when it was
/// asked for; scaled down to fit `room` if the window has shrunk since.
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

/// A side `device` pixels long, in pixels at `scale_factor`.
fn logical_side(device: i32, scale_factor: f32) -> Pixels {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a picture's side is a few thousand pixels at most"
    )]
    let device = device as f32;
    px(device / scale_factor)
}

/// Whether there is an entry before the one at `index` of `len`, and
/// one after it, for Previous and Next.
const fn steps(index: usize, len: usize) -> (bool, bool) {
    (index > 0, index + 1 < len)
}

/// How many lines of text fit a room `height` high: at least one.
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

/// A picture `size` scaled to fit `room`, never up.
fn fitted(size: gpui::Size<Pixels>, room: gpui::Size<Pixels>) -> gpui::Size<Pixels> {
    let scale = fitting_scale(size, room);
    gpui::size(size.width * scale, size.height * scale)
}

/// How much a picture `size` is scaled to fit `room`: never up.
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

/// A text file's first lines, as many as fit the room.
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

/// A folder's first entries, and how many more it holds.
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
                .child(entry_icon(
                    RowIcon::BuiltIn(icon_kind(&entry.name, entry.kind)),
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

/// A card for what is shown, `width` wide at most.
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

/// The large icon of an entry called `name` of `kind`, on a card.
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

/// The caption: the entry's name, kind and size, then Open and Close.
fn caption(
    entry: &Entry,
    current: Option<&Preview>,
    colors: &Colors,
    cx: &Context<MainWindow>,
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
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.open_previewed(window, cx);
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
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.dismiss_preview(window, cx);
                }))
                .child(icon(Icon::Close, size::BUTTON_ICON, colors.text_soft)),
        )
}

/// "PNG image · 1.8 MB", or "Folder · 12 items".
pub(super) fn meta(preview: &Preview) -> String {
    if let PreviewContent::Folder { count, .. } = &preview.content {
        let unit = if *count == 1 { ITEM } else { ITEMS };
        return format!("{FOLDER_KIND}{SEPARATOR}{count} {unit}");
    }
    let kind = preview.kind_name.as_deref().unwrap_or(FILE_KIND);
    format!("{kind}{SEPARATOR}{}", columns::size(preview.bytes))
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
        // A picture with no size is left as it is.
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
        // Even a room too small for one shows one.
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
