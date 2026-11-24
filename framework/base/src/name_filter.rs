//! Typed words found in names: a listing narrowed to the names that hold
//! them, and where they show in a name.

use std::ops::Range;

/// Where `word` first shows in `name`, ignoring case, as a byte range of
/// `name`.
pub fn find_word(name: &str, word: &str) -> Option<Range<usize>> {
    name.char_indices().find_map(|(start, _)| {
        let mut rest = name[start..].char_indices();
        let same = word.chars().all(|wanted| {
            rest.next()
                .is_some_and(|(_, found)| found.to_lowercase().eq(wanted.to_lowercase()))
        });
        let end = rest.next().map_or(name.len(), |(at, _)| start + at);
        same.then_some(start..end)
    })
}

/// Whether `name` holds every word of `filter`, ignoring case. A filter
/// with no words holds in every name.
pub fn holds_every_word(name: &str, filter: &str) -> bool {
    filter
        .split_whitespace()
        .all(|word| find_word(name, word).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_holds_a_filter_when_every_word_shows_in_any_case() {
        assert!(holds_every_word("Holiday Photos.zip", "photo"));
        assert!(holds_every_word("Holiday Photos.zip", "ZIP hol"));
        assert!(!holds_every_word("Holiday Photos.zip", "photo tar"));
        assert!(holds_every_word("Über.txt", "über"));
    }

    #[test]
    fn a_filter_of_only_spaces_holds_in_every_name() {
        assert!(holds_every_word("anything", ""));
        assert!(holds_every_word("anything", "   "));
    }
}
