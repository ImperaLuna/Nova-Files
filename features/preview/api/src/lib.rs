//! Quick looks: what a file or folder shows large, with its name, kind and
//! size beside it.
//!
//! A client asks with [`Command::Preview`]; the core answers
//! [`Event::Previewed`] or [`Event::Unreadable`] with the same
//! [`PreviewId`].

#![forbid(unsafe_code)]

pub mod markdown;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use nova_files_api::{Entry, Fit, Picture, Unreadable};

/// A request from a client to the preview feature's core half.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Look into what is at `path`, following a link, for a quick look; a
    /// picture comes scaled down to `fit`. The answer is
    /// [`Event::Previewed`] or [`Event::Unreadable`] with the same `id`. A
    /// newer `Preview` from the same client cancels this one, which then
    /// may never answer.
    Preview {
        id: PreviewId,
        path: PathBuf,
        fit: Fit,
    },
    /// Draw page `index` of the PDF that [`Command::Preview`] `id` showed
    /// as a [`PreviewContent::Document`], `width` pixels wide. The answer
    /// is [`Event::Paged`] with the same `id` and `index`, unless a newer
    /// preview has replaced `id`.
    Page {
        id: PreviewId,
        index: usize,
        width: u32,
    },
    /// Steer the video or song that [`Command::Preview`] `id` showed as
    /// [`PreviewContent::Media`] and now plays.
    Control { id: PreviewId, control: Control },
    /// The client no longer shows a quick look: the core lets go of what
    /// it holds for the last preview, such as an open PDF.
    Closed,
    /// Open the video or song at `path` in the user's default app, which
    /// starts at `at` when it is a player that can be told so. The answer
    /// is the open feature's `Opened` or `NotOpened` event.
    OpenAt { path: PathBuf, at: Duration },
    /// Draw the picture at `picture`, which Markdown `document` shows, to
    /// fit `fit`. Only a picture inside the document's own folder is drawn.
    /// The answer is [`Event::Drawn`] with the same `id` and `picture`,
    /// unless a newer preview replaced `id` first.
    Draw {
        id: PreviewId,
        document: PathBuf,
        picture: PathBuf,
        fit: Fit,
    },
    /// Draw a Markdown figure from its `source`, at `zoom_percent` of its
    /// own size and no bigger than `fit`. The answer is [`Event::Figured`]
    /// with the same `id` and `source`, unless a newer preview replaced `id`
    /// first.
    Figure {
        id: PreviewId,
        kind: markdown::FigureKind,
        source: Arc<str>,
        zoom_percent: u16,
        fit: Fit,
    },
}

/// Something the preview feature's core half tells one client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// What [`Command::Preview`] `id` shows.
    Previewed {
        id: PreviewId,
        preview: Arc<Preview>,
    },
    /// [`Command::Preview`] `id`'s path couldn't be looked into.
    Unreadable {
        id: PreviewId,
        path: PathBuf,
        reason: Unreadable,
    },
    /// What the video or song of preview `id` is doing, as it plays.
    Played { id: PreviewId, played: Played },
    /// Page `index` of preview `id`'s document, as [`Command::Page`] asked;
    /// `None` when that page couldn't be drawn.
    Paged {
        id: PreviewId,
        index: usize,
        picture: Option<Picture>,
    },
    /// The picture [`Command::Draw`] asked for; `None` when it is outside
    /// the document's folder or couldn't be drawn.
    Drawn {
        id: PreviewId,
        picture: PathBuf,
        drawn: Option<Picture>,
    },
    /// The figure [`Command::Figure`] asked for; `None` when its source
    /// couldn't be drawn.
    Figured {
        id: PreviewId,
        source: Arc<str>,
        drawn: Option<Picture>,
    },
}

/// Names one preview request.
///
/// The client picks it and the core echoes it in the answer. A newer
/// preview from the same client makes the core give up on the older one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PreviewId(pub u64);

/// The most entries a folder's preview names; the rest are only counted.
pub const PREVIEW_ENTRIES: usize = 9;

/// The most bytes of a text file a preview holds: more than a window
/// shows.
pub const PREVIEW_TEXT_BYTES: usize = 32 * 1024;

/// A file or folder as a quick look shows it, answering
/// [`Command::Preview`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub path: PathBuf,
    /// What its type is called in the user's MIME database ("PNG image"),
    /// from its name; `None` for a folder, or a name the database doesn't
    /// know.
    pub kind_name: Option<String>,
    /// Its length in bytes. Always 0 for a folder.
    pub bytes: u64,
    pub content: PreviewContent,
}

/// What a preview shows large.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewContent {
    /// A picture the core could decode, upright, scaled down to the
    /// request's [`Fit`].
    Picture(Picture),
    /// A picture that moves (a GIF, or an animated WebP or PNG): its
    /// frames in order, all one size, scaled down like a
    /// [`PreviewContent::Picture`].
    Moving(Vec<Frame>),
    /// The start of a text file, at most [`PREVIEW_TEXT_BYTES`]; `cut`
    /// when the file goes on past it.
    Text { text: Arc<str>, cut: bool },
    /// The start of a source or config file, as [`PreviewContent::Text`],
    /// with `runs` marking what each piece of it is, in order: their
    /// lengths add up to `text`'s.
    Code {
        text: Arc<str>,
        cut: bool,
        runs: Arc<[Run]>,
    },
    /// A Markdown file read into `blocks`. `text` is its start, at most
    /// [`markdown::PREVIEW_MARKDOWN_BYTES`], `cut` when the file goes on
    /// past it, and `runs` colour `text` as source, like
    /// [`PreviewContent::Code`]'s.
    Markdown {
        text: Arc<str>,
        cut: bool,
        runs: Arc<[Run]>,
        blocks: Arc<[markdown::Block]>,
    },
    /// A PDF: each page's size in points, in order. Its pages come one at a
    /// time through [`Command::Page`].
    Document { pages: Arc<[PageSize]> },
    /// A PDF that needs a password to open.
    Locked,
    /// A video (`video`) or a song, which starts playing as soon as this
    /// answer is sent; what it does comes as [`Event::Played`].
    Media { video: bool },
    /// A folder's first entries, folders first and A to Z, at most
    /// [`PREVIEW_ENTRIES`], and how many it holds in all.
    Folder { first: Vec<Entry>, count: usize },
    /// Nothing the preview can draw: an empty or binary file, a picture
    /// that wouldn't decode, or a device, pipe or socket. The client shows
    /// its icon.
    Icon,
}

/// One piece of a [`PreviewContent::Code`] text: `bytes` long, all of one
/// [`Mark`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub bytes: u32,
    pub mark: Mark,
}

/// What a piece of code is, for its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mark {
    Plain,
    Comment,
    Keyword,
    Quoted,
    Number,
    Function,
    Type,
    Tag,
    Attribute,
}

/// The loudest volume, as a percentage.
pub const MAX_VOLUME: u8 = 100;

/// A change to a playing video or song.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Play,
    Pause,
    /// Jump to this time from the start.
    Seek(Duration),
    /// From 0 to [`MAX_VOLUME`]; later videos and songs start at it too.
    Volume(u8),
    /// Frames from now on fit this box.
    Fit(Fit),
    /// The last [`Played::Frame`] is drawn. No newer frame is sent until
    /// then, so frames never pile up behind a busy window.
    Drawn,
}

/// One thing a playing video or song did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Played {
    /// The newest frame, upright, fitted to the request's box: a video's
    /// picture, or a song's cover. Answer it with [`Control::Drawn`].
    Frame(Picture),
    /// Where playback is now.
    At(Duration),
    /// How long the whole video or song is.
    Length(Duration),
    Paused(bool),
    /// A song's title, artist or album.
    Tag(MediaTag, String),
    /// Playback reached the end and holds the last frame.
    Ended,
    /// Nothing on this computer can play it: libmpv is not installed.
    Unplayable,
}

/// Which of a song's tags a [`Played::Tag`] carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaTag {
    Title,
    Artist,
    Album,
}

/// How big a page of a [`PreviewContent::Document`] is, in points (1/72
/// inch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageSize {
    pub width: u32,
    pub height: u32,
}

/// One frame of a [`PreviewContent::Moving`] picture, shown for `delay`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub picture: Picture,
    pub delay: Duration,
}
