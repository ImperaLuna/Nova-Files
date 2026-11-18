use std::ops::Range;

pub fn matched(name: &str, query: &str) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = query
        .split_whitespace()
        .filter_map(|word| find_ignoring_case(name, word))
        .collect();
    ranges.sort_by_key(|range| range.start);
    let mut joined: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match joined.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => joined.push(range),
        }
    }
    joined
}

fn find_ignoring_case(name: &str, word: &str) -> Option<Range<usize>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_is_marked_where_it_first_shows_ignoring_case() {
        assert_eq!(matched("Tax Return tax.pdf", "tax"), vec![0..3]);
        assert_eq!(matched("my REPORT.pdf", "report"), vec![3..9]);
    }

    #[test]
    fn several_words_are_marked_in_order_and_joined_where_they_touch() {
        assert_eq!(matched("taxes 2024.pdf", "2024 tax"), [0..3, 6..10]);
        assert_eq!(matched("notebook", "note book"), vec![0..8]);
        assert_eq!(matched("notebook", "noteb ebo"), vec![0..6]);
    }

    #[test]
    fn letters_outside_ascii_keep_their_place() {
        let name = "Über straße.txt";
        let marked = matched(name, "STRASSE straße über");
        assert_eq!(marked, [0..5, 6..13]);
        assert_eq!(&name[6..13], "straße");
    }

    #[test]
    fn a_name_found_loosely_shows_no_mark() {
        assert_eq!(matched("report.pdf", "rprt"), Vec::<Range<usize>>::new());
        assert_eq!(matched("report.pdf", "  "), Vec::<Range<usize>>::new());
        assert_eq!(matched("ab", "abc"), Vec::<Range<usize>>::new());
    }
}
