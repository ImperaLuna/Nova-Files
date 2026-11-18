use std::path::Path;

use nova_files_api::uri::{file_uri, path_from_file_uri};
use nova_files_api::{Bookmark, BookmarkChange};

pub const FOLDER: &str = "gtk-3.0";
pub const FILE: &str = "bookmarks";
const LABEL_SEPARATOR: u8 = b' ';
const LINE_END: u8 = b'\n';
const CARRIAGE_RETURN: u8 = b'\r';
const LINE_BREAKS: [char; 2] = ['\n', '\r'];
const LINE_BREAK_STAND_IN: &str = " ";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Lines(Vec<Vec<u8>>);

impl Lines {
    pub fn parse(bytes: &[u8]) -> Self {
        let mut lines: Vec<Vec<u8>> = bytes
            .split(|&byte| byte == LINE_END)
            .map(<[u8]>::to_vec)
            .collect();
        if lines.last().is_some_and(Vec::is_empty) {
            lines.pop();
        }
        Self(lines)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for line in &self.0 {
            bytes.extend_from_slice(line);
            bytes.push(LINE_END);
        }
        bytes
    }

    pub fn bookmarks(&self) -> Vec<Bookmark> {
        self.0.iter().filter_map(|line| parse_line(line)).collect()
    }

    pub fn apply(&mut self, change: &BookmarkChange) -> bool {
        let before = self.0.clone();
        match change {
            BookmarkChange::Add { path, label, at } => {
                if path.is_absolute() && self.find(path).is_none() {
                    let index = self.position(at.and_then(|at| usize::try_from(at).ok()));
                    self.0
                        .insert(index, line_for(file_uri(path).as_bytes(), label.as_deref()));
                }
            }
            BookmarkChange::Remove { path } => {
                self.0
                    .retain(|line| parse_line(line).is_none_or(|found| found.path != *path));
            }
            BookmarkChange::Rename { path, label } => {
                if let Some(index) = self.find(path) {
                    let line = &self.0[index];
                    let line = line.strip_suffix(&[CARRIAGE_RETURN]).unwrap_or(line);
                    let uri = uri_part(line).to_vec();
                    self.0[index] = line_for(&uri, label.as_deref());
                }
            }
            BookmarkChange::Move { path, to } => {
                let count = self.bookmarks().len();
                let to = usize::try_from(*to).unwrap_or(usize::MAX);
                let there = self.pinned_position(path) == Some(to.min(count.saturating_sub(1)));
                if !there && let Some(index) = self.find(path) {
                    let line = self.0.remove(index);
                    let index = self.position(Some(to));
                    self.0.insert(index, line);
                }
            }
        }
        self.0 != before
    }

    fn find(&self, path: &Path) -> Option<usize> {
        self.0
            .iter()
            .position(|line| parse_line(line).is_some_and(|found| found.path == path))
    }

    fn pinned_position(&self, path: &Path) -> Option<usize> {
        self.bookmarks().iter().position(|found| found.path == path)
    }

    fn position(&self, at: Option<usize>) -> usize {
        let pinned = self
            .0
            .iter()
            .enumerate()
            .filter(|(_, line)| parse_line(line).is_some())
            .map(|(index, _)| index);
        at.and_then(|at| pinned.clone().nth(at))
            .unwrap_or(self.0.len())
    }
}

fn parse_line(line: &[u8]) -> Option<Bookmark> {
    let line = line.strip_suffix(&[CARRIAGE_RETURN]).unwrap_or(line);
    let uri = std::str::from_utf8(uri_part(line)).ok()?;
    let path = path_from_file_uri(uri)?;
    let label = line
        .iter()
        .position(|&byte| byte == LABEL_SEPARATOR)
        .map(|space| String::from_utf8_lossy(&line[space + 1..]).into_owned())
        .filter(|label| !label.is_empty());
    Some(Bookmark { path, label })
}

fn uri_part(line: &[u8]) -> &[u8] {
    line.split(|&byte| byte == LABEL_SEPARATOR)
        .next()
        .unwrap_or_default()
}

fn line_for(uri: &[u8], label: Option<&str>) -> Vec<u8> {
    let mut line = uri.to_vec();
    if let Some(label) = label.filter(|label| !label.is_empty()) {
        line.push(LABEL_SEPARATOR);
        line.extend_from_slice(label.replace(LINE_BREAKS, LINE_BREAK_STAND_IN).as_bytes());
    }
    line
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt as _;
    use std::path::PathBuf;

    use super::*;

    fn pinned(path: &str, label: Option<&str>) -> Bookmark {
        Bookmark {
            path: PathBuf::from(path),
            label: label.map(str::to_owned),
        }
    }

    fn apply(text: &str, change: &BookmarkChange) -> (bool, String) {
        let mut lines = Lines::parse(text.as_bytes());
        let changed = lines.apply(change);
        (changed, String::from_utf8(lines.to_bytes()).unwrap())
    }

    #[test]
    fn reads_local_folders_with_their_labels() {
        let text = "file:///home/me/Work\nsftp://host/x Server\n\nfile:///home/me/My%20Stuff Stuff and more\nfile:///home/me/Bare \n";
        assert_eq!(
            Lines::parse(text.as_bytes()).bookmarks(),
            [
                pinned("/home/me/Work", None),
                pinned("/home/me/My Stuff", Some("Stuff and more")),
                pinned("/home/me/Bare", None),
            ]
        );
    }

    #[test]
    fn windows_line_ends_are_read_without_the_carriage_return() {
        let text = "file:///a\r\nfile:///b Bee\r\n";
        assert_eq!(
            Lines::parse(text.as_bytes()).bookmarks(),
            [pinned("/a", None), pinned("/b", Some("Bee"))]
        );
        let rename = BookmarkChange::Rename {
            path: PathBuf::from("/a"),
            label: Some("Ay".to_owned()),
        };
        assert_eq!(
            apply(text, &rename),
            (true, "file:///a Ay\nfile:///b Bee\r\n".to_owned())
        );
    }

    #[test]
    fn names_that_are_not_utf8_keep_their_bytes() {
        let name = OsString::from_vec(b"/tmp/caf\xe9".to_vec());
        let line = file_uri(Path::new(&name));
        assert_eq!(line, "file:///tmp/caf%E9");
        let bookmarks = Lines::parse(line.as_bytes()).bookmarks();
        assert_eq!(bookmarks[0].path.as_os_str(), name);
    }

    #[test]
    fn saving_unchanged_lines_gives_back_the_same_bytes() {
        let text = b"sftp://host/x Server\nfile:///a\n\nnot a uri \xff\n";
        assert_eq!(Lines::parse(text).to_bytes(), text);
        assert_eq!(Lines::parse(b"file:///a").to_bytes(), b"file:///a\n");
        assert_eq!(Lines::parse(b"").to_bytes(), b"");
    }

    #[test]
    fn add_puts_it_last_or_before_a_position() {
        let text = "file:///a\nsftp://host/x\nfile:///b\n";
        let add = |at| BookmarkChange::Add {
            path: PathBuf::from("/c"),
            label: Some("See\nthis".to_owned()),
            at,
        };
        assert_eq!(
            apply(text, &add(None)),
            (true, format!("{text}file:///c See this\n"))
        );
        assert_eq!(
            apply(text, &add(Some(1))),
            (
                true,
                "file:///a\nsftp://host/x\nfile:///c See this\nfile:///b\n".to_owned()
            )
        );
        assert_eq!(
            apply(text, &add(Some(0))),
            (true, format!("file:///c See this\n{text}"))
        );
        assert_eq!(
            apply(text, &add(Some(2))),
            (true, format!("{text}file:///c See this\n"))
        );
    }

    #[test]
    fn add_leaves_a_pinned_or_relative_folder_alone() {
        let text = "file:///a Old\n";
        let again = BookmarkChange::Add {
            path: PathBuf::from("/a"),
            label: Some("New".to_owned()),
            at: None,
        };
        assert_eq!(apply(text, &again), (false, text.to_owned()));
        let relative = BookmarkChange::Add {
            path: PathBuf::from("a"),
            label: None,
            at: None,
        };
        assert_eq!(apply(text, &relative), (false, text.to_owned()));
    }

    #[test]
    fn remove_takes_every_line_for_the_folder() {
        let text = "file:///a\nsftp://host/a\nfile:///a Again\nfile:///b\n";
        let remove = |path: &str| BookmarkChange::Remove {
            path: PathBuf::from(path),
        };
        assert_eq!(
            apply(text, &remove("/a")),
            (true, "sftp://host/a\nfile:///b\n".to_owned())
        );
        assert_eq!(apply(text, &remove("/z")), (false, text.to_owned()));
    }

    #[test]
    fn rename_keeps_the_uri_as_written() {
        let text = "file://localhost/a%2Db Old\nfile:///c\n";
        let rename = |label: Option<&str>| BookmarkChange::Rename {
            path: PathBuf::from("/a-b"),
            label: label.map(str::to_owned),
        };
        assert_eq!(
            apply(text, &rename(Some("New"))),
            (true, "file://localhost/a%2Db New\nfile:///c\n".to_owned())
        );
        assert_eq!(
            apply(text, &rename(None)),
            (true, "file://localhost/a%2Db\nfile:///c\n".to_owned())
        );
        assert_eq!(apply(text, &rename(Some("Old"))), (false, text.to_owned()));
        let missing = BookmarkChange::Rename {
            path: PathBuf::from("/z"),
            label: None,
        };
        assert_eq!(apply(text, &missing), (false, text.to_owned()));
    }

    #[test]
    fn move_counts_pinned_folders_only() {
        let text = "file:///a\nsftp://host/x\nfile:///b\nfile:///c\n";
        let move_to = |path: &str, to| BookmarkChange::Move {
            path: PathBuf::from(path),
            to,
        };
        assert_eq!(
            apply(text, &move_to("/c", 0)),
            (
                true,
                "file:///c\nfile:///a\nsftp://host/x\nfile:///b\n".to_owned()
            )
        );
        assert_eq!(
            apply(text, &move_to("/a", 1)),
            (
                true,
                "sftp://host/x\nfile:///b\nfile:///a\nfile:///c\n".to_owned()
            )
        );
        assert_eq!(
            apply(text, &move_to("/a", 9)),
            (
                true,
                "sftp://host/x\nfile:///b\nfile:///c\nfile:///a\n".to_owned()
            )
        );
        assert_eq!(apply(text, &move_to("/a", 0)), (false, text.to_owned()));
        let last = "file:///a\nsftp://host/x\n";
        assert_eq!(apply(last, &move_to("/a", 0)), (false, last.to_owned()));
        assert_eq!(apply(last, &move_to("/a", 9)), (false, last.to_owned()));
        assert_eq!(apply(text, &move_to("/z", 0)), (false, text.to_owned()));
    }
}
