//! Whether one name matches a query, and how well. Names and the query are
//! compared folded (`fold`), byte by byte. A query of several words matches
//! a name that holds every word, in any order.
//!
//! The kinds, best first:
//! - [`MatchKind::Exact`]: the word is the whole name, or the name without
//!   its extension.
//! - [`MatchKind::WordStart`]: the query appears where a word starts: at
//!   the start of the name, after a separator (` `, `-`, `_`, `.`, …) or
//!   where lower case turns to upper case (`myNotes`).
//! - [`MatchKind::Contains`]: the query appears anywhere else.
//! - [`MatchKind::InOrder`]: the query's letters appear in order, with
//!   others between (`rpt` in `report.pdf`).
//! - [`MatchKind::OneTypo`]: for a query of [`TYPO_MIN`] letters or more,
//!   part of the name is one edit away: a letter missing, swapped with its
//!   neighbour or replaced. (A name with a letter added to the word holds
//!   the word's letters in order, so it is found as that first.) Edits
//!   count bytes, so a letter outside ASCII can take two.

/// The shortest query a typo is forgiven in: shorter ones match too much.
pub const TYPO_MIN: usize = 5;
/// One edit leaves one of the query's first this many letters among the
/// part's first this many.
const EDIT_REACH: usize = 2;

/// How a name matches, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MatchKind {
    Exact,
    WordStart,
    Contains,
    InOrder,
    OneTypo,
}

/// A match and what orders it among matches of its kind: lower is better.
/// For [`MatchKind::InOrder`] it is how far the letters spread, for the
/// rest where the match starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    pub kind: MatchKind,
    pub cost: usize,
}

/// How a name matches every word of a query: the worst word's kind, and
/// the words' costs added up. With `loose`, words may match in order or
/// with a typo too. `name` is the stored name, `folded` the same folded;
/// each word is folded and not empty.
pub fn query(name: &[u8], folded: &[u8], words: &[Vec<u8>], loose: bool) -> Option<Found> {
    let mut all = Found {
        kind: MatchKind::Exact,
        cost: 0,
    };
    for word in words {
        let found = exact(name, folded, word)
            .or_else(|| loose.then(|| self::loose(folded, word)).flatten())?;
        all.kind = all.kind.max(found.kind);
        all.cost += found.cost;
    }
    Some(all)
}

/// [`MatchKind::Exact`] when the word is the whole name, or the name
/// without its extension; else [`MatchKind::WordStart`] or
/// [`MatchKind::Contains`] when it is somewhere in it.
pub fn exact(name: &[u8], folded: &[u8], word: &[u8]) -> Option<Found> {
    let whole = folded == word;
    if whole || stem(folded) == Some(word) {
        return Some(Found {
            kind: MatchKind::Exact,
            cost: 0,
        });
    }
    let mut first = None;
    for at in occurrences(folded, word) {
        if starts_word(name, folded, at) {
            return Some(Found {
                kind: MatchKind::WordStart,
                cost: at,
            });
        }
        first = first.or(Some(at));
    }
    first.map(|at| Found {
        kind: MatchKind::Contains,
        cost: at,
    })
}

/// [`MatchKind::InOrder`] or [`MatchKind::OneTypo`], for a word [`exact`]
/// didn't find.
pub fn loose(folded: &[u8], word: &[u8]) -> Option<Found> {
    if let Some(spread) = in_order(folded, word) {
        return Some(Found {
            kind: MatchKind::InOrder,
            cost: spread,
        });
    }
    one_typo(folded, word).map(|at| Found {
        kind: MatchKind::OneTypo,
        cost: at,
    })
}

/// The name before its last `.`. For a name that starts with its only dot
/// (`.bashrc`) that is empty, which no word is.
fn stem(folded: &[u8]) -> Option<&[u8]> {
    let dot = folded.iter().rposition(|&byte| byte == b'.')?;
    folded.get(..dot)
}

/// Where `query` starts in `folded`, every time.
fn occurrences<'a>(folded: &'a [u8], query: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
    folded
        .windows(query.len())
        .enumerate()
        .filter(move |(_, window)| *window == query)
        .map(|(at, _)| at)
}

/// Whether a word starts at byte `at`. Case changes are read from the
/// stored name, which lines up with the folded one only when it is ASCII.
fn starts_word(name: &[u8], folded: &[u8], at: usize) -> bool {
    let Some(before) = at.checked_sub(1) else {
        return true;
    };
    let separated = folded
        .get(before)
        .is_some_and(|byte| byte.is_ascii() && !byte.is_ascii_alphanumeric());
    let camel = name.is_ascii()
        && name.get(before).is_some_and(u8::is_ascii_lowercase)
        && name.get(at).is_some_and(u8::is_ascii_uppercase);
    separated || camel
}

/// How far apart the first and last letters are when the query's letters
/// appear in order, matched as early as they can be; `None` when they
/// don't.
fn in_order(folded: &[u8], query: &[u8]) -> Option<usize> {
    let mut letters = query.iter();
    let mut wanted = letters.next()?;
    let mut start = None;
    for (at, byte) in folded.iter().enumerate() {
        if byte != wanted {
            continue;
        }
        let first = *start.get_or_insert(at);
        match letters.next() {
            Some(next) => wanted = next,
            None => return Some(at - first),
        }
    }
    None
}

/// Where part of the name one edit away from the query starts, for a long
/// enough query.
fn one_typo(folded: &[u8], query: &[u8]) -> Option<usize> {
    if query.len() < TYPO_MIN {
        return None;
    }
    let shorter = query.len() - 1;
    // Rules out most places at a glance (see EDIT_REACH).
    let starts = query.get(..EDIT_REACH).unwrap_or_default();
    let could_start = |at: usize| {
        folded.get(at..).is_some_and(|rest| {
            rest.iter()
                .take(EDIT_REACH)
                .any(|byte| starts.contains(byte))
        })
    };
    (0..folded.len()).filter(|&at| could_start(at)).find(|&at| {
        (shorter..=query.len()).any(|len| {
            folded
                .get(at..at + len)
                .is_some_and(|part| one_edit(part, query))
        })
    })
}

/// Whether `a` and `b` differ by exactly one edit: an added or missing
/// byte, two neighbours swapped, or one replaced.
fn one_edit(one: &[u8], other: &[u8]) -> bool {
    let (short, long) = if one.len() <= other.len() {
        (one, other)
    } else {
        (other, one)
    };
    let same_start = short
        .iter()
        .zip(long)
        .take_while(|(left, right)| left == right)
        .count();
    let (short_rest, long_rest) = (&short[same_start..], &long[same_start..]);
    match long.len() - short.len() {
        0 => match (short_rest, long_rest) {
            ([first, second, rest @ ..], [swapped_first, swapped_second, more @ ..])
                if first == swapped_second && second == swapped_first && rest == more =>
            {
                true
            }
            ([_, rest @ ..], [_, more @ ..]) => rest == more,
            _ => false,
        },
        1 => long_rest.get(1..) == Some(short_rest),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
