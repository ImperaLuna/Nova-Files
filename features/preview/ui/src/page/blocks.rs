use std::sync::Arc;

use gpui::{
    AnyElement, Div, IntoElement as _, ParentElement as _, Pixels, SharedString, Styled as _, div,
    prelude::FluentBuilder as _, px,
};
use nova_files_preview_api::Run;
use nova_files_preview_api::markdown::{Align, Block, Inline, Item};
use nova_files_shell::assets::Icon;
use nova_files_shell::theme::{Colors, preview, radius, size, text};
use nova_files_shell::widgets::icon;

use super::inline::styled;
use crate::code::Coded;

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

pub struct Paint {
    pub colors: Colors,
    pub mono: SharedString,
}

#[derive(Debug, Clone, Copy)]
pub struct Place {
    pub first: bool,
    pub last: bool,
}

pub fn top_block(block: &Block, place: Place, paint: &Paint) -> AnyElement {
    div()
        .when(place.first, |block| block.pt(preview::TEXT_PAD_Y))
        .pb(if place.last {
            preview::TEXT_PAD_Y
        } else {
            BLOCK_GAP
        })
        .text_size(BODY_SIZE)
        .line_height(BODY_LINE)
        .text_color(paint.colors.text_soft)
        .child(drawn(block, paint))
        .into_any_element()
}

fn drawn(block: &Block, paint: &Paint) -> AnyElement {
    let colors = &paint.colors;
    match block {
        Block::Heading { level, text } => heading(*level, text, colors),
        Block::Paragraph(text) => div().child(styled(text, colors)).into_any_element(),
        Block::Quote(blocks) => stack(blocks, paint)
            .border_l(QUOTE_BAR)
            .border_color(colors.line)
            .pl(QUOTE_PAD)
            .text_color(colors.text_dim)
            .into_any_element(),
        Block::List { first, items } => list(*first, items, paint),
        Block::Code { text, runs } => code(text, runs, paint),
        Block::Figure { source, .. } => code(source, &[], paint),
        Block::Table { aligns, head, rows } => table(aligns, head, rows, colors),
        Block::Rule => div().h(size::LINE).bg(colors.line).into_any_element(),
        Block::Picture { alt, .. } => div()
            .text_color(colors.text_dim)
            .child(SharedString::from(alt.to_string()))
            .into_any_element(),
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

fn heading(level: u8, words: &Inline, colors: &Colors) -> AnyElement {
    let index = usize::from(level.saturating_sub(1)).min(HEADING_SIZES.len() - 1);
    let side = HEADING_SIZES[index];
    let heading = div()
        .text_size(side)
        .line_height(side * HEADING_LINE_SHARE)
        .font_weight(text::STRONG_WEIGHT)
        .text_color(colors.text)
        .child(styled(words, colors));
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
    let colors = &paint.colors;
    let coded = Coded::new(text.trim_end_matches('\n'), runs, paint.mono.clone());
    div()
        .p(CODE_PAD)
        .rounded(radius::ROW)
        .bg(colors.raised)
        .font_family(paint.mono.clone())
        .text_size(CODE_SIZE)
        .line_height(CODE_LINE)
        .text_color(colors.code.plain)
        .whitespace_nowrap()
        .overflow_hidden()
        .children(
            coded
                .lines
                .iter()
                .map(|line| div().truncate().child(line.styled(&colors.code))),
        )
        .into_any_element()
}

fn table(aligns: &[Align], head: &[Inline], rows: &[Arc<[Inline]>], colors: &Colors) -> AnyElement {
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
                cell.children(cells.get(column).map(|words| styled(words, colors)))
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
