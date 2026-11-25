use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{App, Context, Window};
use nova_files_api::uri::{file_uri, path_from_file_uri};
use nova_files_preview_api::Command;

use crate::PreviewPart;
use crate::page::Follow;

const ANCHOR: char = '#';
const QUERY: char = '?';
const SCHEME_END: char = ':';
const FILE_SCHEME: &str = "file";
const MARKDOWN_ENDINGS: [&str; 3] = ["md", "markdown", "mdown"];

#[derive(Debug, PartialEq, Eq)]
pub enum Link {
    Anchor(String),
    Web(String),
    Local(PathBuf),
}

pub fn link(target: &str, folder: &Path) -> Option<Link> {
    let target = target.trim();
    if let Some(anchor) = target.strip_prefix(ANCHOR) {
        return Some(Link::Anchor(anchor.to_owned()));
    }
    match scheme(target) {
        Some(scheme) if scheme.eq_ignore_ascii_case(FILE_SCHEME) => {
            path_from_file_uri(without_tail(target)).map(Link::Local)
        }
        Some(_) => Some(Link::Web(target.to_owned())),
        None => local(without_tail(target), folder).map(Link::Local),
    }
}

fn scheme(target: &str) -> Option<&str> {
    let (scheme, _) = target.split_once(SCHEME_END)?;
    let mut letters = scheme.chars();
    let starts_with_letter = letters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic());
    let rest_fits = letters.all(|letter| letter.is_ascii_alphanumeric() || "+-.".contains(letter));
    (starts_with_letter && rest_fits).then_some(scheme)
}

fn without_tail(target: &str) -> &str {
    target
        .find([ANCHOR, QUERY])
        .map_or(target, |tail| &target[..tail])
}

fn local(target: &str, folder: &Path) -> Option<PathBuf> {
    if target.is_empty() {
        return None;
    }
    let uri = if target.starts_with('/') {
        format!("file://{target}")
    } else {
        format!("{}/{target}", file_uri(folder))
    };
    path_from_file_uri(&uri).map(|path| tidy(&path))
}

fn tidy(path: &Path) -> PathBuf {
    let mut tidied = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                tidied.pop();
            }
            other => tidied.push(other),
        }
    }
    tidied
}

pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ending| ending.to_str())
        .is_some_and(|ending| {
            MARKDOWN_ENDINGS
                .iter()
                .any(|markdown| ending.eq_ignore_ascii_case(markdown))
        })
}

pub fn slug(words: &str) -> String {
    words
        .trim()
        .chars()
        .filter(|letter| letter.is_alphanumeric() || matches!(letter, ' ' | '-' | '_'))
        .flat_map(char::to_lowercase)
        .map(|letter| if letter == ' ' { '-' } else { letter })
        .collect()
}

impl PreviewPart {
    pub(crate) fn follow(cx: &Context<Self>) -> Follow {
        let part = cx.weak_entity();
        Rc::new(move |target: &Arc<str>, _: &mut Window, cx: &mut App| {
            part.update(cx, |part, cx| part.follow_link(target, cx))
                .ok();
        })
    }

    fn follow_link(&self, target: &str, cx: &mut Context<Self>) {
        let Some(folder) = self.shown_folder(cx) else {
            return;
        };
        match link(target, &folder) {
            Some(Link::Anchor(anchor)) => {
                if self.shown_page().is_some_and(|page| page.go_to(&anchor)) {
                    cx.notify();
                }
            }
            Some(Link::Web(address)) => cx.open_url(&address),
            Some(Link::Local(path)) => self.follow_path(path, &folder, cx),
            None => {}
        }
    }

    fn follow_path(&self, path: PathBuf, folder: &Path, cx: &mut Context<Self>) {
        let name = path.file_name().map(OsString::from);
        let here = path.parent() == Some(folder) && is_markdown(&path);
        if let Some(name) = name.filter(|_| here) {
            self.host
                .entries
                .update(cx, |entries, cx| entries.select(vec![name], cx));
            return;
        }
        self.host
            .link
            .send(Command::OpenAt {
                path,
                at: Duration::ZERO,
            })
            .ok();
    }

    fn shown_folder(&self, cx: &App) -> Option<PathBuf> {
        let open = self.looks.open.as_ref()?;
        let listing = self.host.entries.read(cx).listing()?;
        (listing.id == open.listing).then(|| listing.path.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOLDER: &str = "/home/me/notes";

    fn read(target: &str) -> Option<Link> {
        link(target, Path::new(FOLDER))
    }

    #[test]
    fn web_links_and_anchors_are_told_apart_from_files() {
        assert_eq!(read("#install"), Some(Link::Anchor("install".to_owned())));
        assert_eq!(
            read("https://example.org/a?b#c"),
            Some(Link::Web("https://example.org/a?b#c".to_owned()))
        );
        assert_eq!(
            read("mailto:me@example.org"),
            Some(Link::Web("mailto:me@example.org".to_owned()))
        );
    }

    #[test]
    fn local_links_resolve_from_the_documents_folder() {
        let local = |path: &str| Some(Link::Local(PathBuf::from(path)));
        assert_eq!(read("other.md"), local("/home/me/notes/other.md"));
        assert_eq!(read("./a%20b.md#part"), local("/home/me/notes/a b.md"));
        assert_eq!(read("../up/file.pdf?x"), local("/home/me/up/file.pdf"));
        assert_eq!(read("/etc/hosts"), local("/etc/hosts"));
        assert_eq!(read("file:///tmp/x.txt"), local("/tmp/x.txt"));
        assert_eq!(read(""), None);
    }

    #[test]
    fn slugs_follow_the_usual_heading_ids() {
        assert_eq!(slug("Getting Started"), "getting-started");
        assert_eq!(slug("What's new in 2.0?"), "whats-new-in-20");
        assert_eq!(slug("snake_case and-dash"), "snake_case-and-dash");
        assert_eq!(slug("Ünïcode Títle"), "ünïcode-títle");
    }

    #[test]
    fn markdown_endings_are_known() {
        assert!(is_markdown(Path::new("/a/B.MD")));
        assert!(is_markdown(Path::new("/a/b.markdown")));
        assert!(!is_markdown(Path::new("/a/b.txt")));
    }
}
