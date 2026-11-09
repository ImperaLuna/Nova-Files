//! What clients say to the core and what the core says back.

use std::path::PathBuf;
use std::sync::Arc;

use crate::folder::{Details, EntryId, Listing, ListingId, Place, Unreadable};
use crate::job::{JobId, JobTag, Operation, Outcome, Progress};
use crate::search::{Hit, SearchFailed, SearchId};

/// The most entries one [`Command::Rows`] may name: several screens of
/// rows. The core leaves out any past it.
pub const MAX_ROWS: u32 = 512;

/// The most hits one search stage may hold: more than a launcher shows.
/// The core keeps the best ones.
pub const MAX_HITS: u32 = 200;

/// A request from a client to the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Ask the core to answer with [`Event::Pong`], to show it is running.
    Ping,
    /// List the folder at `path`. The answer is [`Event::FolderListed`] or
    /// [`Event::FolderUnreadable`] with the same `id`. A newer `ListFolder`
    /// from the same client cancels this one, which then may never answer.
    ListFolder { id: ListingId, path: PathBuf },
    /// Ask which standard folders exist; the answer is [`Event::Places`].
    Places,
    /// The entries of listing `id` the client shows or is about to show:
    /// `count` entries from `first`. The core reads the details of those it
    /// hasn't read yet, answering with [`Event::DetailsRead`], and makes
    /// their thumbnails, answering each with [`Event::Thumbnail`]. Each
    /// request replaces the one before, so entries scrolled away are
    /// dropped. Entries never asked for never have their details read. At
    /// most [`MAX_ROWS`] entries count.
    Rows {
        id: ListingId,
        first: EntryId,
        count: u32,
    },
    /// Find files under home whose names match `query`, one word or
    /// several separated by spaces. The answer is an [`Event::Found`] for
    /// each stage, best first, at most `limit` hits each (and at most
    /// [`MAX_HITS`]), or [`Event::SearchFailed`]. The first search builds
    /// or opens the home index, so it may take a while; later ones take
    /// milliseconds. A newer `Search` from the same client cancels this
    /// one, which then may stop answering.
    Search {
        id: SearchId,
        query: String,
        limit: u32,
    },
    /// Open the file at `path` in the user's default app for its type.
    /// `activation` is the caller's activation token, so the app may take
    /// focus. The answer is [`Event::FileOpened`] or
    /// [`Event::FileNotOpened`], when the client should ask the desktop
    /// portal instead.
    OpenFile {
        path: PathBuf,
        activation: Option<String>,
    },
    /// Ask for the UI's window state; the answer is [`Event::WindowState`].
    WindowState,
    /// Keep these bytes as the UI's window state, between runs too. The
    /// core never reads them; the UI owns what they say. Bytes over the
    /// core's cap (64 KiB) are dropped.
    SaveWindowState(Vec<u8>),
    /// Hear about every job from now on: first an [`Event::JobRunning`]
    /// for each job already running or paused, then each job's news. A
    /// client that only asks one question and waits for its answer leaves
    /// this out, so no job's news comes in between.
    WatchJobs,
    /// Start `operation` as a job. The answer is [`Event::JobStarted`]
    /// with the same `tag`; this client then watches jobs (see
    /// [`Command::WatchJobs`]) and hears [`Event::JobFinished`] once it
    /// ends.
    StartJob { tag: JobTag, operation: Operation },
    /// Hold job `job` before its next change to the disk. Every client
    /// hears [`Event::JobPaused`].
    PauseJob(JobId),
    /// Let a paused job go on. Every client hears [`Event::JobResumed`].
    ResumeJob(JobId),
    /// Stop job `job` before its next change to the disk, paused or not.
    /// It ends with [`Outcome::Cancelled`].
    CancelJob(JobId),
}

/// Something the core tells one client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The answer to [`Command::Ping`].
    Pong,
    /// A folder's entries, answering [`Command::ListFolder`].
    FolderListed(Listing),
    /// The folder of [`Command::ListFolder`] `id` couldn't be read.
    FolderUnreadable {
        id: ListingId,
        path: PathBuf,
        reason: Unreadable,
    },
    /// Details for entries `first` onward of listing `id`, one per entry,
    /// in entry order, answering [`Command::Rows`]. Each entry's details
    /// come at most once per listing.
    DetailsRead {
        id: ListingId,
        first: EntryId,
        details: Arc<[Details]>,
    },
    /// A thumbnail for entry `entry` of listing `id`, as PNG bytes that fit
    /// in 128 by 128 pixels.
    Thumbnail {
        id: ListingId,
        entry: EntryId,
        png: Arc<[u8]>,
    },
    /// The standard folders that exist, in places-list order.
    Places(Vec<Place>),
    /// One stage of [`Command::Search`] `id`: names as typed come first,
    /// then letters in order and typos. `done` marks the last stage. A path
    /// shows at most once per search.
    Found {
        id: SearchId,
        hits: Vec<Hit>,
        done: bool,
    },
    /// [`Command::Search`] `id` couldn't run.
    SearchFailed { id: SearchId, reason: SearchFailed },
    /// [`Command::OpenFile`]'s app has started.
    FileOpened,
    /// [`Command::OpenFile`] started nothing: sandboxed, no default app for
    /// the file's type, or the app's program couldn't start.
    FileNotOpened,
    /// The UI's window state as last saved, empty when there is none,
    /// answering [`Command::WindowState`].
    WindowState(Vec<u8>),
    /// [`Command::StartJob`] `tag` runs as job `job`. Only the client that
    /// asked hears it, and before anything else about the job.
    JobStarted { tag: JobTag, job: JobId },
    /// Job `job`, started before this client watched jobs, runs (or holds,
    /// when `paused`) doing `operation`, this far along. Answers
    /// [`Command::WatchJobs`].
    JobRunning {
        job: JobId,
        operation: Operation,
        progress: Progress,
        paused: bool,
    },
    /// How far job `job` has got, a few times a second while it runs.
    /// Every client hears it.
    JobProgress { job: JobId, progress: Progress },
    /// Job `job` holds until resumed or cancelled. Every client hears it.
    JobPaused(JobId),
    /// Job `job` goes on. Every client hears it.
    JobResumed(JobId),
    /// Job `job` has ended. `changed` names the folders whose entries it
    /// changed, so a window showing one reads it again. Every client that
    /// watches jobs hears it.
    JobFinished {
        job: JobId,
        outcome: Outcome,
        changed: Vec<PathBuf>,
    },
}

/// Names one connected client, so the core can answer the right one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClientId(pub u64);
