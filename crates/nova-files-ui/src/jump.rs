//! Type to jump: letters typed in a main window move the cursor to the
//! next name that starts with them.
//!
//! Letters typed within [`PAUSE`] of each other build one prefix; a longer
//! pause starts a new one. A prefix is looked for from the cursor's row
//! on, wrapping round to the top, so typing more of the same name keeps
//! the cursor where it is while it still matches. The same letter typed
//! again and again steps through the names that start with it. Names are
//! compared as shown, ignoring case.

use std::ffi::OsStr;
use std::time::{Duration, Instant};

/// How long a pause starts a new prefix.
pub const PAUSE: Duration = Duration::from_secs(1);

/// The letters typed so far.
#[derive(Debug, Default)]
pub struct TypeJump {
    prefix: String,
    last: Option<Instant>,
}

impl TypeJump {
    /// Whether `now` is soon enough after the last letter to add to the
    /// prefix, so a space is part of a name rather than a key of its own.
    pub fn continues(&self, now: Instant) -> bool {
        !self.prefix.is_empty()
            && self
                .last
                .is_some_and(|last| now.saturating_duration_since(last) < PAUSE)
    }

    /// `typed` arrived at `now`: returns the prefix to look for, and
    /// whether to step past the cursor's row (a new prefix, or a letter
    /// repeated).
    pub fn typed(&mut self, typed: &str, now: Instant) -> (String, bool) {
        let fresh = self
            .last
            .is_none_or(|last| now.saturating_duration_since(last) >= PAUSE);
        if fresh {
            self.prefix.clear();
        }
        self.prefix.extend(fold(typed));
        self.last = Some(now);
        let mut letters = self.prefix.chars();
        let first = letters.next();
        let repeated = first.is_some_and(|first| letters.all(|letter| letter == first));
        if repeated && let Some(first) = first {
            return (first.to_string(), true);
        }
        (self.prefix.clone(), false)
    }
}

/// The row of the first name in `names` starting with `prefix` (lower
/// case), looking from `from` on (from the row after it with `past`),
/// wrapping round.
pub fn find<'a>(
    names: impl ExactSizeIterator<Item = &'a OsStr> + Clone,
    prefix: &str,
    from: Option<usize>,
    past: bool,
) -> Option<usize> {
    let len = names.len();
    let start = match from {
        Some(row) if past => row.saturating_add(1),
        Some(row) => row,
        None => 0,
    };
    names
        .enumerate()
        .cycle()
        .skip(start)
        .take(len)
        .find(|(_, name)| starts_with(&name.to_string_lossy(), prefix))
        .map(|(row, _)| row)
}

/// `text` in lower case letter by letter, so a prefix typed a key at a
/// time folds the same as a whole name (no context-dependent forms such as
/// a final sigma).
fn fold(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars().flat_map(char::to_lowercase)
}

/// Whether `name`, folded, starts with the folded `prefix`.
fn starts_with(name: &str, prefix: &str) -> bool {
    let mut name = fold(name);
    prefix.chars().all(|letter| name.next() == Some(letter))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMES: [&str; 5] = ["alpha", "Beta", "bravo", "charlie", "b2"];

    fn at(prefix: &str, from: Option<usize>, past: bool) -> Option<usize> {
        find(NAMES.iter().map(OsStr::new), prefix, from, past)
    }

    #[test]
    fn a_prefix_is_found_from_the_cursor_on_ignoring_case() {
        assert_eq!(at("b", None, false), Some(1));
        assert_eq!(at("br", Some(1), false), Some(2));
        assert_eq!(at("be", Some(1), false), Some(1));
        assert_eq!(at("b", Some(1), true), Some(2));
        assert_eq!(at("b", Some(2), false), Some(2));
        assert_eq!(at("b", Some(4), true), Some(1));
        // Wraps round to the top.
        assert_eq!(at("a", Some(3), true), Some(0));
        assert_eq!(at("z", Some(2), false), None);
    }

    #[test]
    fn letters_close_together_build_a_prefix_and_a_pause_starts_over() {
        let start = Instant::now();
        let mut jump = TypeJump::default();
        assert_eq!(jump.typed("B", start), ("b".to_owned(), true));
        let soon = start + PAUSE / 2;
        assert_eq!(jump.typed("r", soon), ("br".to_owned(), false));
        let later = soon + PAUSE;
        assert_eq!(jump.typed("c", later), ("c".to_owned(), true));
    }

    #[test]
    fn only_a_letter_soon_after_another_continues_the_prefix() {
        let start = Instant::now();
        let mut jump = TypeJump::default();
        assert!(!jump.continues(start));
        jump.typed("m", start);
        assert!(jump.continues(start + PAUSE / 2));
        assert!(!jump.continues(start + PAUSE));
    }

    #[test]
    fn a_name_typed_letter_by_letter_matches_itself() {
        let names = ["ΟΣΑ", "ΟΣ"];
        let start = Instant::now();
        let mut jump = TypeJump::default();
        jump.typed("Ο", start);
        let (prefix, past) = jump.typed("Σ", start);
        let found = find(names.iter().map(OsStr::new), &prefix, Some(1), past);
        assert_eq!(found, Some(1));
        assert!(!starts_with("ab", "abc"));
    }

    #[test]
    fn a_repeated_letter_steps_through_its_names() {
        let start = Instant::now();
        let mut jump = TypeJump::default();
        jump.typed("b", start);
        assert_eq!(jump.typed("b", start), ("b".to_owned(), true));
        assert_eq!(jump.typed("b", start), ("b".to_owned(), true));
    }
}
