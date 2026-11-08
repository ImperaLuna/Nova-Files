//! What the pick bar's name box asks for, worked out from its text alone:
//! a folder to go to, a path to try as a folder, or a name in the folder
//! on screen. Also the names a save picks: the part of a name before its
//! extension, and a free numbered name to keep both files.

use std::ffi::{OsStr, OsString};
use std::ops::Range;
use std::path::{Path, PathBuf};

/// The home folder's short form.
const HOME: &str = "~";
/// What separates folders in a typed path.
const SEPARATOR: char = '/';
/// What starts an extension.
const DOT: char = '.';
/// Extensions that go with the one after them, as in `.tar.gz`.
const DOUBLE_EXTENSIONS: [&str; 1] = [".tar"];
/// The first number a kept copy gets: the original is the first.
const FIRST_COPY: u32 = 2;

/// What the name box's text asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Typed {
    /// Nothing typed.
    Nothing,
    /// A path that ends in `/`, or `~`: go there.
    Folder(PathBuf),
    /// A path: go there if it is a folder, else to the folder it is in,
    /// with its last part as the name.
    Path(PathBuf),
    /// A name in the folder on screen.
    Name(String),
}

/// Reads `text` as typed in the folder `folder`, with `~` for `home`.
pub fn read(text: &str, home: &Path, folder: &Path) -> Typed {
    let text = text.trim();
    if text.is_empty() {
        return Typed::Nothing;
    }
    let expanded = match text.strip_prefix(HOME) {
        Some("") => return Typed::Folder(home.to_path_buf()),
        Some(rest) if rest.starts_with(SEPARATOR) => home.join(rest.trim_start_matches(SEPARATOR)),
        _ if text.contains(SEPARATOR) => folder.join(text),
        _ => return Typed::Name(text.to_owned()),
    };
    if text.ends_with(SEPARATOR) {
        Typed::Folder(expanded)
    } else {
        Typed::Path(expanded)
    }
}

/// Where `name`'s extension starts, if it has one: at its last dot, or the
/// dot before a double extension like `.tar.gz`. A leading dot, as in
/// `.bashrc`, starts no extension.
fn extension_start(name: &str) -> Option<usize> {
    let last = name.rfind(DOT).filter(|&dot| dot > 0)?;
    let start = DOUBLE_EXTENSIONS
        .iter()
        .find(|double| name[..last].ends_with(*double))
        .map_or(last, |double| last - double.len());
    Some(if start > 0 { start } else { last })
}

/// The part of `name` before its extension, as a byte range, for a save
/// box to select so typing replaces just the name.
pub fn stem(name: &str) -> Range<usize> {
    0..extension_start(name).unwrap_or(name.len())
}

/// The first of `name (2).ext`, `name (3).ext`, ... that `taken` says is
/// free, to save beside a file named `name`. A name that isn't UTF-8 gets
/// its number at the end, so its own bytes stay as they are.
pub fn keep_both(name: &OsStr, taken: impl Fn(&OsStr) -> bool) -> OsString {
    let numbered = |copy: u32| {
        name.to_str().map_or_else(
            || {
                let mut numbered = name.to_owned();
                numbered.push(format!(" ({copy})"));
                numbered
            },
            |text| {
                let (stem, extension) = text.split_at(stem(text).end);
                OsString::from(format!("{stem} ({copy}){extension}"))
            },
        )
    };
    (FIRST_COPY..u32::MAX)
        .map(numbered)
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| name.to_owned())
}

#[cfg(test)]
mod tests;
