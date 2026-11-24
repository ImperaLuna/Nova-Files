pub const TYPO_MIN: usize = 5;
const EDIT_REACH: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MatchKind {
    Exact,
    WordStart,
    Contains,
    InOrder,
    OneTypo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    pub kind: MatchKind,
    pub cost: usize,
}

pub fn exact(folded: &[u8], word: &[u8], starts_word: impl Fn(usize) -> bool) -> Option<Found> {
    let whole = folded == word;
    if whole || stem(folded) == Some(word) {
        return Some(Found {
            kind: MatchKind::Exact,
            cost: 0,
        });
    }
    let mut first = None;
    for at in occurrences(folded, word) {
        if starts_word(at) {
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

fn stem(folded: &[u8]) -> Option<&[u8]> {
    let dot = folded.iter().rposition(|&byte| byte == b'.')?;
    folded.get(..dot)
}

pub fn contains(folded: &[u8], word: &[u8]) -> bool {
    occurrences(folded, word).next().is_some()
}

fn occurrences<'a>(folded: &'a [u8], query: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
    let (first, rest) = query.split_first().unzip();
    let rest = rest.unwrap_or_default();
    folded
        .iter()
        .enumerate()
        .filter(move |&(at, byte)| {
            Some(byte) == first
                && folded
                    .get(at + 1..)
                    .is_some_and(|after| after.starts_with(rest))
        })
        .map(|(at, _)| at)
}

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

fn one_typo(folded: &[u8], query: &[u8]) -> Option<usize> {
    if query.len() < TYPO_MIN {
        return None;
    }
    let shorter = query.len() - 1;
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
