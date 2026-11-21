use std::path::{Path, PathBuf};

pub use crate::paths::HOME;
const SEPARATOR: char = '/';
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

#[cfg(test)]
mod tests;
