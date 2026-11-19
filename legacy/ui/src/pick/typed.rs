use std::ffi::{OsStr, OsString};
use std::ops::Range;
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};

pub use nova_files_shell::paths::HOME;
const SEPARATOR: char = '/';
const DOT: char = '.';
const DOUBLE_EXTENSIONS: [&str; 1] = [".tar"];
const FIRST_COPY: u32 = 2;
const NAME_MAX: usize = 255;
const DOT_FOLDERS: [&str; 2] = [".", ".."];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Typed {
    Nothing,
    Folder(PathBuf),
    Path(PathBuf),
    Name(String),
}

pub fn read(text: &str, home: &Path, folder: &Path) -> Typed {
    let text = text.trim();
    if text.is_empty() {
        return Typed::Nothing;
    }
    let expanded = match text.strip_prefix(HOME) {
        Some("") => return Typed::Folder(home.to_path_buf()),
        Some(rest) if rest.starts_with(SEPARATOR) => home.join(rest.trim_start_matches(SEPARATOR)),
        _ if text.contains(SEPARATOR) || DOT_FOLDERS.contains(&text) => folder.join(text),
        _ => return Typed::Name(text.to_owned()),
    };
    if text.ends_with(SEPARATOR) || DOT_FOLDERS.contains(&text) {
        Typed::Folder(expanded)
    } else {
        Typed::Path(expanded)
    }
}

fn extension_start(name: &str) -> Option<usize> {
    let last = name.rfind(DOT).filter(|&dot| dot > 0)?;
    let start = DOUBLE_EXTENSIONS
        .iter()
        .find(|double| name[..last].ends_with(*double))
        .map_or(last, |double| last - double.len());
    Some(if start > 0 { start } else { last })
}

pub fn stem(name: &str) -> Range<usize> {
    0..extension_start(name).unwrap_or(name.len())
}

pub fn keep_both(name: &OsStr, taken: impl Fn(&OsStr) -> bool) -> OsString {
    let numbered = |copy: u32| {
        let number = format!(" ({copy})");
        name.to_str().map_or_else(
            || {
                let room = NAME_MAX.saturating_sub(number.len());
                let bytes = name.as_bytes();
                let mut numbered = bytes[..bytes.len().min(room)].to_vec();
                numbered.extend_from_slice(number.as_bytes());
                OsString::from_vec(numbered)
            },
            |text| {
                let (stem, extension) = text.split_at(stem(text).end);
                let room = NAME_MAX.saturating_sub(number.len() + extension.len());
                let stem = &stem[..stem.floor_char_boundary(room)];
                OsString::from(format!("{stem}{number}{extension}"))
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
