//! Each kind on its own, then queries of several words.

use super::*;
use crate::search::fold::fold_into;

/// Where matches start, and how far letters in order spread, in the
/// examples below.
const START: usize = 0;
const SECOND_WORD: usize = 7;
const MIDDLE: usize = 2;
const AFTER_MY: usize = 3;
const SPREAD: usize = 5;
/// `rpt` in `my report.pdf`: from the r at 3 to the t at 8.
const LATER_SPREAD: usize = 5;

fn found(name: &str, query: &str, loose: bool) -> Option<Found> {
    let mut folded = Vec::new();
    fold_into(name.as_bytes(), &mut folded);
    let words: Vec<Vec<u8>> = query
        .split_whitespace()
        .map(|word| {
            let mut out = Vec::new();
            fold_into(word.as_bytes(), &mut out);
            out
        })
        .collect();
    self::query(name.as_bytes(), &folded, &words, loose)
}

fn kind(name: &str, query: &str) -> Option<MatchKind> {
    found(name, query, true).map(|found| found.kind)
}

#[test]
fn the_whole_name_or_its_stem_is_exact() {
    assert_eq!(kind("Report.pdf", "report.PDF"), Some(MatchKind::Exact));
    assert_eq!(kind("Report.pdf", "report"), Some(MatchKind::Exact));
    assert_eq!(
        kind("archive.tar.gz", "archive.tar"),
        Some(MatchKind::Exact)
    );
    // A leading dot is the name, not an extension.
    assert_eq!(kind(".bashrc", ".bashrc"), Some(MatchKind::Exact));
    assert_eq!(kind("Café", "cafe"), Some(MatchKind::Exact));
}

#[test]
fn a_word_starts_at_the_start_a_separator_or_a_capital() {
    assert_eq!(
        found("tax return.pdf", "tax", false),
        Some(Found {
            kind: MatchKind::WordStart,
            cost: START
        })
    );
    assert_eq!(
        found("my-tax return", "return", false),
        Some(Found {
            kind: MatchKind::WordStart,
            cost: SECOND_WORD
        })
    );
    assert_eq!(kind("myTaxes", "tax"), Some(MatchKind::WordStart));
    assert_eq!(kind("2024_taxes", "tax"), Some(MatchKind::WordStart));
    // A later occurrence at a word start beats an earlier one inside a word.
    assert_eq!(kind("syntax tax", "tax"), Some(MatchKind::WordStart));
}

#[test]
fn inside_a_word_is_contains() {
    assert_eq!(
        found("syntax", "nta", false),
        Some(Found {
            kind: MatchKind::Contains,
            cost: MIDDLE
        })
    );
    // Café folds to plain letters, so its stored case can't mark a word.
    assert_eq!(kind("ÉcoleNormale", "normale"), Some(MatchKind::Contains));
}

#[test]
fn letters_in_order_only_when_loose() {
    assert_eq!(found("report.pdf", "rpt", false), None);
    assert_eq!(
        found("report.pdf", "rpt", true),
        Some(Found {
            kind: MatchKind::InOrder,
            cost: SPREAD
        })
    );
    assert_eq!(
        found("my report.pdf", "rpt", true),
        Some(Found {
            kind: MatchKind::InOrder,
            cost: LATER_SPREAD
        })
    );
    assert_eq!(kind("report.pdf", "tpr"), None);
}

#[test]
fn one_typo_in_a_long_enough_word() {
    // Replaced, missing, added, swapped.
    assert_eq!(kind("holiday photos", "photus"), Some(MatchKind::OneTypo));
    assert_eq!(kind("holiday photos", "photoz"), Some(MatchKind::OneTypo));
    assert_eq!(kind("holiday photos", "phtos"), Some(MatchKind::InOrder));
    assert_eq!(kind("holiday", "holidaay"), Some(MatchKind::OneTypo));
    assert_eq!(kind("holiday", "hloiday"), Some(MatchKind::OneTypo));
    assert_eq!(kind("holiday", "hoilday"), Some(MatchKind::OneTypo));
    // The shortest word a typo is forgiven in.
    assert_eq!(kind("photo", "photz"), Some(MatchKind::OneTypo));
    // Two edits, or a short word.
    assert_eq!(kind("holiday", "hlidya"), None);
    assert_eq!(kind("cats", "cass"), None);
    assert_eq!(found("holiday", "hloiday", false), None);
}

#[test]
fn every_word_must_match_and_the_worst_one_counts() {
    assert_eq!(
        kind("nova-benchmark", "nova bench"),
        Some(MatchKind::WordStart)
    );
    assert_eq!(
        kind("nova-benchmark", "bench nova"),
        Some(MatchKind::WordStart)
    );
    assert_eq!(
        kind("nova-benchmark", "nova chm"),
        Some(MatchKind::Contains)
    );
    assert_eq!(
        kind("nova-benchmark", "nova bnch"),
        Some(MatchKind::InOrder)
    );
    assert_eq!(kind("nova-benchmark", "nova zed"), None);
    assert_eq!(found("nova-benchmark", "nova bnch", false), None);
    let both = found("my-tax return", "tax return", false).unwrap();
    assert_eq!(both.cost, AFTER_MY + SECOND_WORD);
}

#[test]
fn one_edit_means_exactly_one() {
    assert!(one_edit(b"photos", b"photus"));
    assert!(one_edit(b"photos", b"photo"));
    assert!(one_edit(b"photo", b"photos"));
    assert!(one_edit(b"photos", b"hpotos"));
    assert!(!one_edit(b"photos", b"photos"));
    assert!(one_edit(b"photos", b"phoots"));
    assert!(!one_edit(b"photos", b"phtoso"));
    assert!(!one_edit(b"photos", b"pho"));
    assert!(!one_edit(b"abcd", b"badc"));
}
