use super::*;

fn blocks(text: &str) -> Vec<Block> {
    read(text, || false).unwrap().0
}

fn words(inline: &Inline) -> &str {
    &inline.text
}

fn looks(inline: &Inline) -> Vec<(String, Look)> {
    let mut at = 0;
    inline
        .spans
        .iter()
        .map(|span| {
            let end = at + span.bytes as usize;
            let piece = inline.text[at..end].to_owned();
            at = end;
            (piece, span.look)
        })
        .collect()
}

#[test]
fn headings_and_paragraphs_keep_their_order() {
    let blocks = blocks("# Title\n\nSome words\nwrapped.\n\n### Small\n");
    let [
        Block::Heading {
            level: 1,
            text: title,
        },
        Block::Paragraph(body),
        Block::Heading {
            level: 3,
            text: small,
        },
    ] = blocks.as_slice()
    else {
        panic!("{blocks:?}");
    };
    assert_eq!(words(title), "Title");
    assert_eq!(words(body), "Some words wrapped.");
    assert_eq!(words(small), "Small");
}

#[test]
fn spans_cover_the_text_and_carry_their_look() {
    let blocks = blocks("a **bold _both_** `code` ~~gone~~ $x^2$\n");
    let [Block::Paragraph(text)] = blocks.as_slice() else {
        panic!("{blocks:?}");
    };
    let covered: u32 = text.spans.iter().map(|span| span.bytes).sum();
    assert_eq!(covered as usize, text.text.len());
    let looks = looks(text);
    let look_of = |piece: &str| looks.iter().find(|(text, _)| text == piece).unwrap().1;
    assert_eq!(look_of("bold "), Look::STRONG);
    assert_eq!(look_of("both"), Look::STRONG.with(Look::EMPHASIS));
    assert_eq!(look_of("code"), Look::CODE);
    assert_eq!(look_of("gone"), Look::STRUCK);
    assert_eq!(look_of("x^2"), Look::FORMULA);
}

#[test]
fn links_name_their_target() {
    let blocks = blocks("See [the docs](docs/guide.md) now.\n");
    let [Block::Paragraph(text)] = blocks.as_slice() else {
        panic!("{blocks:?}");
    };
    let linked = text.spans.iter().find(|span| span.link.is_some()).unwrap();
    assert_eq!(linked.link.as_deref(), Some("docs/guide.md"));
    assert_eq!(linked.bytes as usize, "the docs".len());
}

#[test]
fn lists_nest_and_tasks_are_ticked() {
    let blocks = blocks("3. one\n4. two\n   - [x] done\n   - [ ] open\n");
    let [
        Block::List {
            first: Some(3),
            items,
        },
    ] = blocks.as_slice()
    else {
        panic!("{blocks:?}");
    };
    assert_eq!(items.len(), 2);
    let [
        Block::Paragraph(two),
        Block::List {
            first: None,
            items: tasks,
        },
    ] = &*items[1].blocks
    else {
        panic!("{:?}", items[1]);
    };
    assert_eq!(words(two), "two");
    let ticks: Vec<_> = tasks.iter().map(|item| item.checked).collect();
    assert_eq!(ticks, [Some(true), Some(false)]);
}

#[test]
fn quotes_hold_their_blocks() {
    let blocks = blocks("> # Said\n> words\n");
    let [Block::Quote(inside)] = blocks.as_slice() else {
        panic!("{blocks:?}");
    };
    assert!(matches!(
        &**inside,
        [Block::Heading { .. }, Block::Paragraph(_)]
    ));
}

#[test]
fn fences_are_coloured_or_drawn_as_figures() {
    let blocks = blocks(
        "```rust\nlet x = 1;\n```\n\n```\nplain\n```\n\n```mermaid\nflowchart TD\n  A --> B\n```\n",
    );
    let [
        Block::Code { text: rust, runs },
        Block::Code { runs: plain, .. },
        Block::Figure {
            kind: FigureKind::Diagram,
            source,
        },
    ] = blocks.as_slice()
    else {
        panic!("{blocks:?}");
    };
    assert_eq!(&**rust, "let x = 1;\n");
    assert!(!runs.is_empty());
    assert!(plain.is_empty());
    assert!(source.starts_with("flowchart TD"));
}

#[test]
fn a_formula_on_its_own_line_is_a_figure() {
    let blocks = blocks("Before\n\n$$\na + b\n$$\n\nAfter\n");
    let [
        Block::Paragraph(_),
        Block::Figure {
            kind: FigureKind::Formula,
            source,
        },
        Block::Paragraph(_),
    ] = blocks.as_slice()
    else {
        panic!("{blocks:?}");
    };
    assert_eq!(source.trim(), "a + b");
}

#[test]
fn a_picture_splits_its_paragraph() {
    let blocks = blocks("Look:\n![the screen](shots/one.png)\nNice.\n");
    let [
        Block::Paragraph(before),
        Block::Picture {
            target,
            alt,
            link: None,
        },
        Block::Paragraph(after),
    ] = blocks.as_slice()
    else {
        panic!("{blocks:?}");
    };
    assert_eq!(words(before).trim(), "Look:");
    assert_eq!(&**target, "shots/one.png");
    assert_eq!(&**alt, "the screen");
    assert_eq!(words(after).trim(), "Nice.");
}

#[test]
fn a_picture_in_a_heading_reads_as_its_words() {
    let blocks = blocks("# Hi ![logo](logo.png)\n");
    let [Block::Heading { text, .. }] = blocks.as_slice() else {
        panic!("{blocks:?}");
    };
    assert_eq!(words(text), "Hi logo");
}

#[test]
fn tables_keep_columns_and_rows() {
    let blocks = blocks("| Name | Size |\n|:-----|-----:|\n| a | 1 |\n| b | 2 |\n");
    let [Block::Table { aligns, head, rows }] = blocks.as_slice() else {
        panic!("{blocks:?}");
    };
    assert_eq!(&**aligns, [Align::Start, Align::End]);
    assert_eq!(words(&head[1]), "Size");
    assert_eq!(rows.len(), 2);
    assert_eq!(words(&rows[1][0]), "b");
}

#[test]
fn footnotes_are_referenced_and_kept() {
    let blocks = blocks("Said[^why] and[^how].\n\n[^how]: Thus.\n\n[^why]: Where.\n");
    let [
        Block::Paragraph(text),
        Block::Note { label, blocks },
        Block::Note { label: second, .. },
    ] = blocks.as_slice()
    else {
        panic!("{blocks:?}");
    };
    assert!(looks(text).contains(&("[1]".to_owned(), Look::NOTE)));
    assert!(looks(text).contains(&("[2]".to_owned(), Look::NOTE)));
    assert_eq!(&**label, "2");
    assert_eq!(&**second, "1");
    assert!(matches!(&**blocks, [Block::Paragraph(_)]));
}

#[test]
fn front_matter_is_left_out() {
    let blocks = blocks("---\ntitle: x\n---\n\n# Body\n");
    assert!(matches!(blocks.as_slice(), [Block::Heading { .. }]));
}

#[test]
fn deep_nesting_stops_at_a_floor() {
    let text = format!("{}deep\n", "> ".repeat(DEEPEST * 4));
    let blocks = blocks(&text);
    let mut depth = 0;
    let mut level = blocks.as_slice();
    while let [Block::Quote(inside)] = level {
        depth += 1;
        level = inside;
    }
    assert_eq!(depth, DEEPEST);
    assert!(matches!(level, [Block::Paragraph(_)]));
}

#[test]
fn a_file_cut_in_the_middle_keeps_what_was_read() {
    let blocks = blocks("- one\n- two\n  > quoted and cut");
    let [Block::List { items, .. }] = blocks.as_slice() else {
        panic!("{blocks:?}");
    };
    assert_eq!(items.len(), 2);
}

#[test]
fn a_cancelled_read_gives_up() {
    assert!(read("# Title\n", || true).is_none());
}

#[test]
fn fences_past_the_colouring_budget_stay_plain() {
    let text = "```rust\nlet x = 1;\n```\n\n".repeat(FENCES_COLOURED + 1);
    let blocks = blocks(&text);
    let coloured = |block: &Block| matches!(block, Block::Code { runs, .. } if !runs.is_empty());
    assert!(blocks.first().is_some_and(coloured));
    assert!(blocks.last().is_some_and(|last| !coloured(last)));
}

#[test]
fn html_shows_only_its_words() {
    let blocks =
        blocks("<!-- TOC -->\n\n<p align=\"center\"><b>Fast</b></p>\n\nPress <kbd>Esc</kbd>.\n");
    let [Block::Paragraph(html), Block::Paragraph(press)] = blocks.as_slice() else {
        panic!("{blocks:?}");
    };
    assert_eq!(words(html), "Fast");
    assert_eq!(words(press), "Press Esc.");
}

#[test]
fn a_linked_picture_keeps_its_link_and_whole_alt() {
    let blocks = blocks("[![`ci` status](badge.svg)](https://ci.example)\n");
    let [Block::Picture { alt, link, .. }] = blocks.as_slice() else {
        panic!("{blocks:?}");
    };
    assert_eq!(&**alt, "ci status");
    assert_eq!(link.as_deref(), Some("https://ci.example"));
}

#[test]
fn lists_nest_as_deep_as_quotes() {
    let text: String = (0..DEEPEST)
        .flat_map(|level| ["  ".repeat(level), "- level\n".to_owned()])
        .collect();
    let mut depth = 0;
    let blocks = blocks(&text);
    let mut level = blocks.as_slice();
    while let [Block::List { items, .. }] = level {
        depth += 1;
        level = items.last().map_or(&[], |item| {
            item.blocks.last().map_or(&[], std::slice::from_ref)
        });
    }
    assert_eq!(depth, DEEPEST);
}
