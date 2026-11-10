//! Files on the clipboard, in the formats other file managers and apps
//! read and write, so files cut or copied here paste there, and back.
//!
//! A cut or copy offers the same files several ways, each under its MIME
//! type: GNOME's `x-special/gnome-copied-files` (`copy` or `cut`, then one
//! `file://` URI per line, which Nautilus, Nemo, Thunar and Caja use), the
//! standard `text/uri-list` (one URI per line, ended by CRLF, which Dolphin
//! and most apps use), KDE's `application/x-kde-cutselection` (`1` on a
//! cut) and plain text (one path per line, for pasting into a text field).
//!
//! A paste reads the richest format offered: GNOME's, then the URI list
//! with KDE's cut mark, then plain text. Plain text counts as files only
//! as Nautilus 3.30 to 42 wrote them (`x-special/nautilus-clipboard`, then
//! GNOME's format) or as nothing but `file://` URIs: a path typed or copied
//! as text is text, so pasting it never starts a copy. Only local files
//! count: a URI for another machine is left out, and a paste with no local
//! file left is no paste. Paths are bytes, so a name that isn't valid
//! UTF-8 goes through every format but plain text whole.
//!
//! The window owns the system clipboard; these are only the bytes it puts
//! there and takes back. A paste becomes a copy or move job
//! ([`FileClipboard::paste_into`]).

use std::path::PathBuf;

use crate::job::Operation;
use crate::uri::{file_uri, path_from_file_uri};

/// GNOME's format, which also says whether the files were cut.
pub const GNOME_COPIED_FILES: &str = "x-special/gnome-copied-files";
/// The standard list of URIs (RFC 2483).
pub const URI_LIST: &str = "text/uri-list";
/// KDE's mark that the files in [`URI_LIST`] were cut.
pub const KDE_CUT_SELECTION: &str = "application/x-kde-cutselection";
/// Plain text, as a text field takes it.
pub const PLAIN_TEXT_UTF8: &str = "text/plain;charset=utf-8";
pub const PLAIN_TEXT: &str = "text/plain";

const COPY_WORD: &str = "copy";
const CUT_WORD: &str = "cut";
/// KDE's cut mark: the files were cut.
const KDE_CUT: &str = "1";
/// The first line of the plain text Nautilus 3.30 to 42 put on the
/// clipboard, before GNOME's format.
const NAUTILUS_MARK: &str = "x-special/nautilus-clipboard";
/// RFC 2483 ends each line of a URI list with CRLF.
const URI_LIST_LINE_END: &str = "\r\n";
const URI_LIST_COMMENT: char = '#';
const LINE_END: char = '\n';
const LINE_END_TEXT: &str = "\n";
const CARRIAGE_RETURN: char = '\r';

/// Whether the files were cut (they move when pasted) or copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipboardAction {
    Copy,
    Cut,
}

/// Files someone cut or copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileClipboard {
    pub action: ClipboardAction,
    pub paths: Vec<PathBuf>,
}

/// One format the clipboard offers: its MIME type and its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub mime: String,
    pub bytes: Vec<u8>,
}

impl Offer {
    pub fn new(mime: &str, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            mime: mime.to_owned(),
            bytes: bytes.into(),
        }
    }
}

impl FileClipboard {
    /// What to put on the clipboard: each format, richest first. Only
    /// absolute paths can be offered; with none, there is nothing to offer.
    pub fn offers(&self) -> Vec<Offer> {
        let paths: Vec<&PathBuf> = self
            .paths
            .iter()
            .filter(|path| path.is_absolute())
            .collect();
        if paths.is_empty() {
            return Vec::new();
        }
        let uris: Vec<String> = paths.iter().map(|path| file_uri(path)).collect();
        let word = match self.action {
            ClipboardAction::Copy => COPY_WORD,
            ClipboardAction::Cut => CUT_WORD,
        };
        let gnome = format!("{word}{LINE_END}{}", uris.join(LINE_END_TEXT));
        let uri_list: String = uris
            .iter()
            .flat_map(|uri| [uri.as_str(), URI_LIST_LINE_END])
            .collect();
        let text = paths
            .iter()
            .map(|path| path.to_string_lossy())
            .collect::<Vec<_>>()
            .join(LINE_END_TEXT)
            .into_bytes();
        let mut offers = vec![
            Offer::new(GNOME_COPIED_FILES, gnome),
            Offer::new(URI_LIST, uri_list),
        ];
        if self.action == ClipboardAction::Cut {
            offers.push(Offer::new(KDE_CUT_SELECTION, KDE_CUT));
        }
        offers.push(Offer::new(PLAIN_TEXT_UTF8, text.clone()));
        offers.push(Offer::new(PLAIN_TEXT, text));
        offers
    }

    /// The files `offers` hold, from the richest format there; `None`
    /// when no format holds a local file.
    pub fn from_offers(offers: &[Offer]) -> Option<Self> {
        let find = |mime: &str| {
            offers
                .iter()
                .find(|offer| offer.mime.eq_ignore_ascii_case(mime))
                .and_then(|offer| std::str::from_utf8(&offer.bytes).ok())
        };
        if let Some(files) = find(GNOME_COPIED_FILES).and_then(from_gnome) {
            return Some(files);
        }
        if let Some(paths) = find(URI_LIST).and_then(from_uri_list) {
            let cut = find(KDE_CUT_SELECTION).is_some_and(|mark| mark.trim() == KDE_CUT);
            let action = if cut {
                ClipboardAction::Cut
            } else {
                ClipboardAction::Copy
            };
            return Some(Self { action, paths });
        }
        find(PLAIN_TEXT_UTF8)
            .or_else(|| find(PLAIN_TEXT))
            .and_then(from_text)
    }

    /// The job that pastes these files into the folder `into`: a move
    /// for cut files, a copy for copied ones. Cut files already in `into`
    /// stay where they are, so a cut pasted back where it came from is no
    /// job (`None`). A copy into its own folder is left to the job, which
    /// asks what to do as for any name that's taken.
    pub fn paste_into(&self, into: PathBuf) -> Option<Operation> {
        match self.action {
            ClipboardAction::Copy => Some(Operation::Copy {
                sources: self.paths.clone(),
                into,
            }),
            ClipboardAction::Cut => {
                let sources: Vec<PathBuf> = self
                    .paths
                    .iter()
                    .filter(|path| path.parent() != Some(into.as_path()))
                    .cloned()
                    .collect();
                (!sources.is_empty()).then_some(Operation::Move { sources, into })
            }
        }
    }
}

/// The lines of `text`, without line ends (LF or CRLF) or empty lines.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split(LINE_END)
        .map(|line| line.strip_suffix(CARRIAGE_RETURN).unwrap_or(line))
        .filter(|line| !line.is_empty())
}

/// The local paths among `uris`; `None` when there are none.
fn local<'a>(uris: impl Iterator<Item = &'a str>) -> Option<Vec<PathBuf>> {
    let paths: Vec<PathBuf> = uris.filter_map(path_from_file_uri).collect();
    (!paths.is_empty()).then_some(paths)
}

fn from_gnome(text: &str) -> Option<FileClipboard> {
    let mut lines = lines(text);
    let action = match lines.next()? {
        COPY_WORD => ClipboardAction::Copy,
        CUT_WORD => ClipboardAction::Cut,
        _ => return None,
    };
    Some(FileClipboard {
        action,
        paths: local(lines)?,
    })
}

fn from_uri_list(text: &str) -> Option<Vec<PathBuf>> {
    local(lines(text).filter(|line| !line.starts_with(URI_LIST_COMMENT)))
}

/// Plain text as Nautilus 3.30 to 42 wrote files, or nothing but local
/// `file://` URIs, one a line (copied files); any other text isn't files.
fn from_text(text: &str) -> Option<FileClipboard> {
    let mut rest = lines(text);
    if rest.next()? == NAUTILUS_MARK {
        let gnome: Vec<&str> = rest.collect();
        return from_gnome(&gnome.join(LINE_END_TEXT));
    }
    let paths: Option<Vec<PathBuf>> = lines(text).map(path_from_file_uri).collect();
    Some(FileClipboard {
        action: ClipboardAction::Copy,
        paths: paths?,
    })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt as _;

    use super::*;

    fn files(action: ClipboardAction, paths: &[&str]) -> FileClipboard {
        FileClipboard {
            action,
            paths: paths.iter().map(PathBuf::from).collect(),
        }
    }

    fn one(mime: &str, text: &str) -> Vec<Offer> {
        vec![Offer::new(mime, text)]
    }

    fn text_of(offers: &[Offer], mime: &str) -> Option<String> {
        offers
            .iter()
            .find(|offer| offer.mime == mime)
            .map(|offer| String::from_utf8(offer.bytes.clone()).unwrap())
    }

    #[test]
    fn a_cut_offers_every_format() {
        let offers = files(ClipboardAction::Cut, &["/h/a b", "/h/c"]).offers();
        let mimes: Vec<&str> = offers.iter().map(|offer| offer.mime.as_str()).collect();
        assert_eq!(
            mimes,
            [
                GNOME_COPIED_FILES,
                URI_LIST,
                KDE_CUT_SELECTION,
                PLAIN_TEXT_UTF8,
                PLAIN_TEXT
            ]
        );
        assert_eq!(
            text_of(&offers, GNOME_COPIED_FILES).unwrap(),
            "cut\nfile:///h/a%20b\nfile:///h/c"
        );
        assert_eq!(
            text_of(&offers, URI_LIST).unwrap(),
            "file:///h/a%20b\r\nfile:///h/c\r\n"
        );
        assert_eq!(text_of(&offers, KDE_CUT_SELECTION).unwrap(), "1");
        assert_eq!(text_of(&offers, PLAIN_TEXT).unwrap(), "/h/a b\n/h/c");
    }

    #[test]
    fn a_copy_has_no_cut_mark() {
        let offers = files(ClipboardAction::Copy, &["/h/a"]).offers();
        assert_eq!(
            text_of(&offers, GNOME_COPIED_FILES).unwrap(),
            "copy\nfile:///h/a"
        );
        assert_eq!(text_of(&offers, KDE_CUT_SELECTION), None);
    }

    #[test]
    fn what_is_offered_reads_back_the_same() {
        for action in [ClipboardAction::Copy, ClipboardAction::Cut] {
            let name = OsString::from_vec(b"/h/caf\xe9".to_vec());
            let mut cut = files(action, &["/h/a b", "/h/%25"]);
            cut.paths.push(PathBuf::from(name));
            assert_eq!(FileClipboard::from_offers(&cut.offers()), Some(cut));
        }
    }

    #[test]
    fn reads_gnomes_format_from_other_apps() {
        assert_eq!(
            FileClipboard::from_offers(&one(
                GNOME_COPIED_FILES,
                "cut\r\nfile:///h/a\r\nsftp://host/x\nfile:///h/b\n"
            )),
            Some(files(ClipboardAction::Cut, &["/h/a", "/h/b"]))
        );
        assert_eq!(
            FileClipboard::from_offers(&one(GNOME_COPIED_FILES, "copy\nfile:///h/a")),
            Some(files(ClipboardAction::Copy, &["/h/a"]))
        );
        for refused in ["move\nfile:///h/a", "copy\n", "copy\nsftp://host/x", ""] {
            assert_eq!(
                FileClipboard::from_offers(&one(GNOME_COPIED_FILES, refused)),
                None,
                "{refused:?}"
            );
        }
    }

    #[test]
    fn reads_a_uri_list_with_kdes_cut_mark() {
        let list = "# from Dolphin\r\nfile:///h/a\r\nfile:///h/b\r\n";
        assert_eq!(
            FileClipboard::from_offers(&one(URI_LIST, list)),
            Some(files(ClipboardAction::Copy, &["/h/a", "/h/b"]))
        );
        let mut cut = one(URI_LIST, list);
        cut.push(Offer::new("Application/X-KDE-CutSelection", "1\n"));
        assert_eq!(
            FileClipboard::from_offers(&cut),
            Some(files(ClipboardAction::Cut, &["/h/a", "/h/b"]))
        );
        let mut not_cut = one(URI_LIST, list);
        not_cut.push(Offer::new(KDE_CUT_SELECTION, "0"));
        assert_eq!(
            FileClipboard::from_offers(&not_cut).map(|files| files.action),
            Some(ClipboardAction::Copy)
        );
    }

    #[test]
    fn the_richest_format_wins() {
        let mut offers = one(URI_LIST, "file:///h/from-list\r\n");
        offers.push(Offer::new(GNOME_COPIED_FILES, "cut\nfile:///h/from-gnome"));
        assert_eq!(
            FileClipboard::from_offers(&offers),
            Some(files(ClipboardAction::Cut, &["/h/from-gnome"]))
        );
        // A format with nothing local falls through to the next.
        let mut offers = one(GNOME_COPIED_FILES, "copy\nsmb://host/x");
        offers.push(Offer::new(PLAIN_TEXT, "file:///h/a"));
        assert_eq!(
            FileClipboard::from_offers(&offers),
            Some(files(ClipboardAction::Copy, &["/h/a"]))
        );
    }

    #[test]
    fn plain_text_counts_only_when_every_line_is_a_file_uri() {
        assert_eq!(
            FileClipboard::from_offers(&one(PLAIN_TEXT_UTF8, "file:///h/a\nfile:///h/b%20c\n")),
            Some(files(ClipboardAction::Copy, &["/h/a", "/h/b c"]))
        );
        // A path copied as text is text: pasting it starts no copy.
        for text in [
            "hello world",
            "/h/a",
            "file:///h/a\nnotes",
            "",
            "\n\n",
            "relative/path",
        ] {
            assert_eq!(
                FileClipboard::from_offers(&one(PLAIN_TEXT, text)),
                None,
                "{text:?}"
            );
        }
        // Bytes that aren't text are no paste.
        assert_eq!(
            FileClipboard::from_offers(&[Offer::new(PLAIN_TEXT, b"file:///h/\xff".to_vec())]),
            None
        );
    }

    #[test]
    fn reads_the_plain_text_nautilus_42_writes() {
        let text = "x-special/nautilus-clipboard\ncut\nfile:///h/a\nfile:///h/b%20c\n";
        assert_eq!(
            FileClipboard::from_offers(&one(PLAIN_TEXT_UTF8, text)),
            Some(files(ClipboardAction::Cut, &["/h/a", "/h/b c"]))
        );
        for refused in [
            "x-special/nautilus-clipboard\nmove\nfile:///h/a",
            "x-special/nautilus-clipboard\ncopy\n",
            "x-special/nautilus-clipboard",
        ] {
            assert_eq!(
                FileClipboard::from_offers(&one(PLAIN_TEXT, refused)),
                None,
                "{refused:?}"
            );
        }
    }

    #[test]
    fn only_absolute_paths_are_offered() {
        let offers = files(ClipboardAction::Copy, &["h/a", "/h/b"]).offers();
        assert_eq!(
            text_of(&offers, GNOME_COPIED_FILES).unwrap(),
            "copy\nfile:///h/b"
        );
        assert_eq!(files(ClipboardAction::Cut, &["h/a"]).offers(), Vec::new());
        assert_eq!(files(ClipboardAction::Cut, &[]).offers(), Vec::new());
    }

    #[test]
    fn a_paste_is_a_copy_or_a_move() {
        let into = PathBuf::from("/h/into");
        assert_eq!(
            files(ClipboardAction::Copy, &["/h/a"]).paste_into(into.clone()),
            Some(Operation::Copy {
                sources: vec![PathBuf::from("/h/a")],
                into: into.clone(),
            })
        );
        assert_eq!(
            files(ClipboardAction::Cut, &["/h/a", "/h/into/b"]).paste_into(into.clone()),
            Some(Operation::Move {
                sources: vec![PathBuf::from("/h/a")],
                into: into.clone(),
            })
        );
        // A cut pasted back where it came from moves nothing.
        assert_eq!(
            files(ClipboardAction::Cut, &["/h/into/b"]).paste_into(into),
            None
        );
    }
}
