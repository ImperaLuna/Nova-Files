//! Whether a file fits one of an app's file filters, by its name.
//!
//! A filter's globs match the name, ignoring case, as GTK's own dialog
//! does. Its MIME types need the system's MIME database, which the core
//! reads; a [`FileTypes`] hands the answers over without the caller
//! touching the disk. A name the database knows nothing about can't be
//! judged without reading the file, so a filter with MIME types lets it
//! through rather than block a file that may be fine.

use std::ffi::OsStr;
use std::fmt;

use crate::pick::{FileFilter, FilterRule};

/// The MIME types of file names, from the system's MIME database.
pub trait FileTypes: Send + Sync + fmt::Debug {
    /// The type `name` says it is, if the database knows its pattern.
    fn of(&self, name: &OsStr) -> Option<&str>;

    /// Whether `mime` is `wanted`, or a kind of it: an alias of it, a
    /// subclass of it (`text/markdown` of `text/plain`), or any type of a
    /// `wanted` media like `image/*`.
    fn is_a(&self, mime: &str, wanted: &str) -> bool;
}

impl FileFilter {
    /// Whether a file called `name` fits this filter, with `types` for its
    /// MIME rules (without it, they let every file through).
    pub fn admits(&self, name: &OsStr, types: Option<&dyn FileTypes>) -> bool {
        let text = name.to_string_lossy();
        let mut wants_type = false;
        for rule in &self.rules {
            match rule {
                FilterRule::Glob(pattern) => {
                    if glob_matches(pattern, &text, false) {
                        return true;
                    }
                }
                FilterRule::MimeType(_) => wants_type = true,
            }
        }
        if !wants_type {
            return false;
        }
        let Some(mime) = types.and_then(|types| types.of(name)) else {
            return true;
        };
        self.rules.iter().any(|rule| match rule {
            FilterRule::MimeType(wanted) => types.is_some_and(|types| types.is_a(mime, wanted)),
            FilterRule::Glob(_) => false,
        })
    }
}

/// Whether `name` matches the shell glob `pattern`: `*` is any run of
/// characters, `?` one character, `[abc]`, `[a-z]` and `[!a]` one of a set.
/// A `[` with no closing `]` is a plain character.
pub fn glob_matches(pattern: &str, name: &str, case_sensitive: bool) -> bool {
    let fold = |text: &str| -> Vec<char> {
        if case_sensitive {
            text.chars().collect()
        } else {
            text.chars().flat_map(char::to_lowercase).collect()
        }
    };
    matches(&fold(pattern), &fold(name))
}

const ANY_RUN: char = '*';
const ANY_ONE: char = '?';
const SET_OPEN: char = '[';
const SET_CLOSE: char = ']';
const SET_NOT: char = '!';
const SET_RANGE: char = '-';
/// Where a set's `]` can first be: one right after `[` is in the set.
const FIRST_CLOSE_AT: usize = 2;

/// The glob match itself, backtracking to the last `*` on a miss.
fn matches(pattern: &[char], name: &[char]) -> bool {
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        let step = match pattern.get(p) {
            Some(&ANY_RUN) => {
                star = Some((p, n));
                p += 1;
                continue;
            }
            Some(&ANY_ONE) => Some(1),
            Some(&SET_OPEN) => set(&pattern[p..], name[n]),
            Some(&literal) => (literal == name[n]).then_some(1),
            None => None,
        };
        match (step, star) {
            (Some(width), _) => {
                p += width;
                n += 1;
            }
            // Back to the `*`, which takes one more character this time.
            (None, Some((star_p, star_n))) => {
                p = star_p;
                n = star_n + 1;
            }
            (None, None) => return false,
        }
    }
    pattern[p..].iter().all(|&rest| rest == ANY_RUN)
}

/// How far a `[...]` set at the start of `pattern` reaches if it holds
/// `c`, or `None` if it doesn't. An unclosed `[` matches only itself.
fn set(pattern: &[char], c: char) -> Option<usize> {
    // After `[!`, too, a `]` first in the set is one of its members.
    let first_close = FIRST_CLOSE_AT + usize::from(pattern.get(1) == Some(&SET_NOT));
    let Some(close) = pattern
        .iter()
        .enumerate()
        .skip(first_close)
        .find_map(|(at, &item)| (item == SET_CLOSE).then_some(at))
    else {
        return (c == SET_OPEN).then_some(1);
    };
    let body = &pattern[1..close];
    let (negated, body) = match body.split_first() {
        Some((&SET_NOT, rest)) => (true, rest),
        _ => (false, body),
    };
    let mut holds = false;
    let mut rest = body;
    while let Some((&first, after)) = rest.split_first() {
        if let [SET_RANGE, last, tail @ ..] = after {
            holds |= (first..=*last).contains(&c);
            rest = tail;
        } else {
            holds |= first == c;
            rest = after;
        }
    }
    (holds != negated).then_some(close + 1)
}

#[cfg(test)]
mod tests;
