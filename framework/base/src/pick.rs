//! Pick requests: another app asks, through the file picker portal, for
//! files to open or a place to save one.
//!
//! The D-Bus side builds a [`PickRequest`] and hands it to the UI, which
//! answers through the request's [`PickReply`] once the user has picked or
//! cancelled. A reply dropped unanswered (the window closed, the program
//! stopped) answers [`PickAnswer::Cancelled`], so the asking app always
//! hears back. The portal may also close a request first; the UI then gets
//! the request's [`PickId`] to close its window.

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use crate::file_type::FileTypes;

/// One pick request, unique while the program runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PickId(pub u64);

/// What the app asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickKind {
    /// Files to open, or folders when `folders` is set; more than one when
    /// `multiple` is.
    Open { multiple: bool, folders: bool },
    /// A path to save one file at, suggested `name` first. A file name
    /// on Linux is any bytes, so the name an app suggests keeps its own.
    Save { name: OsString },
}

/// The asking app's window, which a picker opens over, as the portal names
/// it in `parent_window`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParentWindow {
    /// A handle the app exported through Wayland's xdg-foreign protocol.
    Wayland(String),
    /// An X11 window id.
    X11(u32),
}

impl ParentWindow {
    /// The portal's `parent_window` string: `wayland:` and a handle, or
    /// `x11:` and the window id in hex. Empty or anything else names no
    /// window, which the portal allows.
    pub fn parse(text: &str) -> Option<Self> {
        if let Some(handle) = text.strip_prefix("wayland:") {
            return (!handle.is_empty()).then(|| Self::Wayland(handle.to_owned()));
        }
        let id = text.strip_prefix("x11:")?;
        let id = id.strip_prefix("0x").unwrap_or(id);
        u32::from_str_radix(id, HEX).ok().map(Self::X11)
    }
}

/// The base of an X11 window id in `parent_window`.
const HEX: u32 = 16;

/// One rule of a [`FileFilter`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterRule {
    /// A shell glob on the name, such as `*.pdf`.
    Glob(String),
    /// A MIME type, such as `image/png` or `image/*`.
    MimeType(String),
}

/// A named set of file types the app offers, such as "PDF documents".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFilter {
    pub name: String,
    pub rules: Vec<FilterRule>,
}

/// What the user chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickAnswer {
    /// The paths picked (one to save at), with the filter in use, as an
    /// index into the request's filters.
    Picked {
        paths: Vec<PathBuf>,
        filter: Option<usize>,
    },
    /// The user cancelled, or the window closed.
    Cancelled,
}

/// Answers a [`PickRequest`] once. Dropped unanswered, it answers
/// [`PickAnswer::Cancelled`].
pub struct PickReply(Option<Box<dyn FnOnce(PickAnswer) + Send>>);

impl PickReply {
    /// A reply that hands the answer to `deliver`.
    pub fn new(deliver: impl FnOnce(PickAnswer) + Send + 'static) -> Self {
        Self(Some(Box::new(deliver)))
    }

    /// Sends `answer`.
    pub fn answer(mut self, answer: PickAnswer) {
        if let Some(deliver) = self.0.take() {
            deliver(answer);
        }
    }
}

impl Drop for PickReply {
    fn drop(&mut self) {
        if let Some(deliver) = self.0.take() {
            deliver(PickAnswer::Cancelled);
        }
    }
}

impl fmt::Debug for PickReply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PickReply")
    }
}

/// An app's request for a pick.
#[derive(Debug)]
pub struct PickRequest {
    pub id: PickId,
    pub kind: PickKind,
    /// The window title the app asks for.
    pub title: String,
    /// The label the app asks for on the Open or Save button.
    pub accept_label: Option<String>,
    pub filters: Vec<FileFilter>,
    /// The filter in use at first, as an index into `filters`.
    pub filter: Option<usize>,
    /// Where to start; the home folder when `None`.
    pub folder: Option<PathBuf>,
    /// The app's window to open over, when it names one.
    pub parent: Option<ParentWindow>,
    /// The MIME database for the filters' MIME rules, once it is read.
    pub types: Option<Arc<dyn FileTypes>>,
    pub reply: PickReply,
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::{ParentWindow, PickAnswer, PickReply};

    #[test]
    fn the_portal_names_a_wayland_or_x11_window() {
        assert_eq!(
            ParentWindow::parse("wayland:2a9c-handle"),
            Some(ParentWindow::Wayland("2a9c-handle".to_owned()))
        );
        assert_eq!(
            ParentWindow::parse("x11:4a00007"),
            Some(ParentWindow::X11(0x04a0_0007))
        );
        assert_eq!(
            ParentWindow::parse("x11:0x1F"),
            Some(ParentWindow::X11(0x1f))
        );
        for none in [
            "",
            "wayland:",
            "x11:",
            "x11:zz",
            "x11:100000000",
            "4a00007",
            "mir:1",
        ] {
            assert_eq!(ParentWindow::parse(none), None, "{none:?}");
        }
    }

    #[test]
    fn a_reply_answers_once() {
        let (sender, answers) = mpsc::channel();
        let reply = PickReply::new(move |answer| sender.send(answer).unwrap());

        reply.answer(PickAnswer::Picked {
            paths: vec!["/tmp/a".into()],
            filter: None,
        });

        assert_eq!(
            answers.try_iter().collect::<Vec<_>>(),
            [PickAnswer::Picked {
                paths: vec!["/tmp/a".into()],
                filter: None
            }]
        );
    }

    #[test]
    fn a_reply_shows_only_its_name_in_debug_output() {
        assert_eq!(format!("{:?}", PickReply::new(drop)), "PickReply");
    }

    #[test]
    fn a_reply_dropped_unanswered_cancels() {
        let (sender, answers) = mpsc::channel();
        drop(PickReply::new(move |answer| sender.send(answer).unwrap()));

        assert_eq!(
            answers.try_iter().collect::<Vec<_>>(),
            [PickAnswer::Cancelled]
        );
    }
}
