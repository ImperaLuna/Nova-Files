use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use gpui::{
    AnyElement, Div, IntoElement as _, ParentElement as _, Pixels, SharedString, Styled as _, div,
    img, prelude::FluentBuilder as _, px,
};
use nova_files_preview_api::Run;
use nova_files_preview_api::markdown::{Align, Block, Inline, Item};
use nova_files_shell::assets::Icon;
use nova_files_shell::theme::{Colors, preview, radius, size, text};
use nova_files_shell::widgets::icon;

use super::Follow;
use super::inline::words;
use super::pictures::Drawn;
use crate::code::Coded;
use crate::draw::logical_side;
use crate::links::{Link, link};

const BLOCK_GAP: Pixels = px(12.0);
const BODY_SIZE: Pixels = px(14.5);
const BODY_LINE: Pixels = px(23.0);
const HEADING_SIZES: [Pixels; 6] = [px(26.0), px(21.0), px(18.0), px(16.0), px(14.5), px(13.5)];
const HEADING_LINE_SHARE: f32 = 1.3;
const RULED_HEADINGS: u8 = 2;
const HEADING_RULE_GAP: Pixels = px(6.0);
const QUOTE_BAR: Pixels = px(3.0);
const QUOTE_PAD: Pixels = px(14.0);
const MARKER_WIDTH: Pixels = px(26.0);
const ITEM_GAP: Pixels = px(4.0);
const BULLET: &str = "•";
const NUMBER_END: &str = ".";
const TICK_BOX: Pixels = px(15.0);
const TICK_ICON: Pixels = px(11.0);
const TICK_DROP: Pixels = px(4.0);
const CODE_PAD: Pixels = px(14.0);
const CODE_SIZE: Pixels = px(13.0);
const CODE_LINE: Pixels = px(20.0);
const CELL_PAD_X: Pixels = px(10.0);
const CELL_PAD_Y: Pixels = px(6.0);
const NOTE_SIZE: Pixels = px(12.5);
const NOTE_LABEL_WIDTH: Pixels = px(26.0);
const LINE_END: char = '\n';
const PICTURE_GAP: Pixels = px(8.0);
const PICTURE_ICON: Pixels = px(16.0);
const HOST_START: &str = "://";
const ROOT: &str = "/";

pub struct Paint {
    pub colors: Colors,
    pub mono: SharedString,
    pub follow: Follow,
    pub pictures: Drawn,
    pub room: (Pixels, f32),
}

#[derive(Debug, Clone, Copy)]
pub struct Place {
    pub first: bool,
    pub last: bool,
}

pub fn top_block(block: &Block, place: Place, paint: &Paint) -> AnyElement {
    div()
        .when(place.first, |block| block.pt(preview::TEXT_PAD_Y))
        .pb(gap_after(place))
        .text_size(BODY_SIZE)
        .line_height(BODY_LINE)
        .text_color(paint.colors.text_soft)
        .child(drawn(block, paint))
        .into_any_element()
}

const fn gap_after(place: Place) -> Pixels {
    if place.last {
        preview::TEXT_PAD_Y
    } else {
        BLOCK_GAP
    }
}

fn drawn(block: &Block, paint: &Paint) -> AnyElement {
    let colors = &paint.colors;
    match block {
        Block::Heading { level, text } => heading(*level, text, paint),
        Block::Paragraph(text) => div().child(words(text, paint)).into_any_element(),
        Block::Quote(blocks) => stack(blocks, paint)
            .border_l(QUOTE_BAR)
            .border_color(colors.line)
            .pl(QUOTE_PAD)
            .text_color(colors.text_dim)
            .into_any_element(),
        Block::List { first, items } => list(*first, items, paint),
        Block::Code { text, runs } => code(text, runs, paint),
        Block::Figure { source, .. } => code(source, &[], paint),
        Block::Table { aligns, head, rows } => table(aligns, head, rows, paint),
        Block::Rule => div().h(size::LINE).bg(colors.line).into_any_element(),
        Block::Picture { target, alt, .. } => picture(target, alt, paint),
        Block::Note { label, blocks } => note(label, blocks, paint),
    }
}

fn stack(blocks: &[Block], paint: &Paint) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(BLOCK_GAP)
        .children(blocks.iter().map(|block| drawn(block, paint)))
}

fn heading(level: u8, text: &Inline, paint: &Paint) -> AnyElement {
    let colors = &paint.colors;
    let index = usize::from(level.saturating_sub(1)).min(HEADING_SIZES.len() - 1);
    let side = HEADING_SIZES[index];
    let heading = div()
        .text_size(side)
        .line_height(side * HEADING_LINE_SHARE)
        .font_weight(text::STRONG_WEIGHT)
        .text_color(colors.text)
        .child(words(text, paint));
    if level <= RULED_HEADINGS {
        heading
            .pb(HEADING_RULE_GAP)
            .border_b(size::LINE)
            .border_color(colors.line)
            .into_any_element()
    } else {
        heading.into_any_element()
    }
}

fn list(first: Option<u64>, items: &[Item], paint: &Paint) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(ITEM_GAP)
        .children(items.iter().zip(0_u64..).map(|(item, index)| {
            let marker = match (item.checked, first) {
                (Some(checked), _) => tick(checked, &paint.colors),
                (None, Some(first)) => {
                    let number = first.saturating_add(index);
                    marker_text(format!("{number}{NUMBER_END}"), &paint.colors)
                }
                (None, None) => marker_text(BULLET.to_owned(), &paint.colors),
            };
            div()
                .flex()
                .child(div().flex_none().w(MARKER_WIDTH).child(marker))
                .child(stack(&item.blocks, paint).flex_1().min_w_0().gap(ITEM_GAP))
        }))
        .into_any_element()
}

fn marker_text(marker: String, colors: &Colors) -> AnyElement {
    div()
        .text_color(colors.text_dim)
        .child(SharedString::from(marker))
        .into_any_element()
}

fn tick(checked: bool, colors: &Colors) -> AnyElement {
    let other = div()
        .mt(TICK_DROP)
        .size(TICK_BOX)
        .flex()
        .items_center()
        .justify_center()
        .rounded(radius::TICK)
        .border(size::LINE);
    if checked {
        other
            .border_color(colors.accept)
            .bg(colors.accept)
            .child(icon(Icon::Check, TICK_ICON, colors.on_accept))
            .into_any_element()
    } else {
        other.border_color(colors.text_dim).into_any_element()
    }
}

fn code(text: &str, runs: &[Run], paint: &Paint) -> AnyElement {
    let coded = Coded::new(text.trim_end_matches(LINE_END), runs, paint.mono.clone());
    code_box(paint)
        .p(CODE_PAD)
        .rounded(radius::ROW)
        .children(code_lines(&coded, 0..coded.lines.len(), paint))
        .into_any_element()
}

pub fn code_rows(coded: &Coded, lines: Range<usize>, place: Place, paint: &Paint) -> AnyElement {
    let first_lines = lines.start == 0;
    let last_lines = lines.end == coded.lines.len();
    let code = code_box(paint)
        .px(CODE_PAD)
        .when(first_lines, |code| code.pt(CODE_PAD).rounded_t(radius::ROW))
        .when(last_lines, |code| code.pb(CODE_PAD).rounded_b(radius::ROW))
        .children(code_lines(coded, lines, paint));
    div()
        .when(place.first && first_lines, |row| {
            row.pt(preview::TEXT_PAD_Y)
        })
        .when(last_lines, |row| row.pb(gap_after(place)))
        .child(code)
        .into_any_element()
}

fn code_box(paint: &Paint) -> Div {
    div()
        .bg(paint.colors.raised)
        .font_family(paint.mono.clone())
        .text_size(CODE_SIZE)
        .line_height(CODE_LINE)
        .text_color(paint.colors.code.plain)
        .whitespace_nowrap()
        .overflow_hidden()
}

fn code_lines<'a>(
    coded: &'a Coded,
    lines: Range<usize>,
    paint: &'a Paint,
) -> impl Iterator<Item = Div> + 'a {
    coded
        .lines
        .get(lines)
        .unwrap_or_default()
        .iter()
        .map(|line| div().truncate().child(line.styled(&paint.colors.code)))
}

fn table(aligns: &[Align], head: &[Inline], rows: &[Arc<[Inline]>], paint: &Paint) -> AnyElement {
    let colors = &paint.colors;
    let row = |cells: &[Inline]| {
        div()
            .flex()
            .border_b(size::LINE)
            .border_color(colors.line)
            .children(aligns.iter().enumerate().map(|(column, align)| {
                let cell = div().flex_1().min_w_0().px(CELL_PAD_X).py(CELL_PAD_Y);
                let cell = match align {
                    Align::Start => cell.text_left(),
                    Align::Center => cell.text_center(),
                    Align::End => cell.text_right(),
                };
                cell.children(cells.get(column).map(|text| words(text, paint)))
            }))
    };
    div()
        .flex()
        .flex_col()
        .rounded(radius::ROW)
        .border(size::LINE)
        .border_color(colors.line)
        .overflow_hidden()
        .child(
            row(head)
                .bg(colors.raised)
                .text_color(colors.text)
                .font_weight(text::STRONG_WEIGHT),
        )
        .children(rows.iter().map(|cells| row(cells)))
        .into_any_element()
}

fn note(label: &str, blocks: &[Block], paint: &Paint) -> AnyElement {
    div()
        .flex()
        .text_size(NOTE_SIZE)
        .text_color(paint.colors.text_dim)
        .child(
            div()
                .flex_none()
                .min_w(NOTE_LABEL_WIDTH)
                .child(SharedString::from(format!("{label}{NUMBER_END}"))),
        )
        .child(stack(blocks, paint).flex_1().min_w_0())
        .into_any_element()
}

fn picture(target: &Arc<str>, alt: &str, paint: &Paint) -> AnyElement {
    let Some(image) = paint.pictures.get(target) else {
        return unshown(target, alt, &paint.colors);
    };
    let (room, scale_factor) = paint.room;
    let side = image.size(0);
    let (width, height) = (
        logical_side(side.width.0, scale_factor),
        logical_side(side.height.0, scale_factor),
    );
    let shown = width.min(room);
    let share = if width > Pixels::ZERO {
        shown / width
    } else {
        0.0
    };
    img(Arc::clone(image))
        .w(shown)
        .h(height * share)
        .rounded(radius::ROW)
        .into_any_element()
}

fn unshown(target: &str, alt: &str, colors: &Colors) -> AnyElement {
    let host = match link(target, Path::new(ROOT)) {
        Some(Link::Web(address)) => host(&address),
        _ => None,
    };
    div()
        .flex()
        .items_center()
        .gap(PICTURE_GAP)
        .px(CELL_PAD_X)
        .py(CELL_PAD_Y)
        .rounded(radius::ROW)
        .bg(colors.raised)
        .text_color(colors.text_dim)
        .child(icon(Icon::Pictures, PICTURE_ICON, colors.text_dim))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(SharedString::from(alt.to_owned())),
        )
        .children(host.map(|host| div().flex_none().child(SharedString::from(host))))
        .into_any_element()
}

fn host(address: &str) -> Option<String> {
    let (_, rest) = address.split_once(HOST_START)?;
    let host = rest.split(['/', '?', '#']).next()?;
    (!host.is_empty()).then(|| host.to_owned())
}
