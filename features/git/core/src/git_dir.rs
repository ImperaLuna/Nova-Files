use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};

pub const DOT_GIT: &str = ".git";
const POINTER: &str = "gitdir:";
const COMMON: &str = "commondir";
const LONGEST_POINTER: u64 = 4096;

pub fn git_dir(folder: &Path) -> Option<PathBuf> {
    let dot = folder.join(DOT_GIT);
    if fs::metadata(&dot).ok()?.is_dir() {
        return Some(dot);
    }
    let text = short_text(&dot)?;
    let target = text.lines().next()?.strip_prefix(POINTER)?.trim();
    Some(folder.join(target))
}

pub fn common_dir(git_dir: &Path) -> Option<PathBuf> {
    let text = short_text(&git_dir.join(COMMON))?;
    Some(git_dir.join(text.lines().next()?.trim()))
}

fn short_text(path: &Path) -> Option<String> {
    let mut text = String::new();
    File::open(path)
        .ok()?
        .take(LONGEST_POINTER)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}
