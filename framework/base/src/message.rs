//! What clients say to the core and what the core says back.

use std::path::PathBuf;
use std::sync::Arc;

use crate::file_icon::FileIcon;
use crate::folder::{Details, EntryId, Listing, ListingId, Order, Unreadable};
use crate::job::{Answer, Conflict, JobId, JobTag, Operation, Outcome, Progress, Scope, Tally};
use crate::thumbnail_size::ThumbnailSize;

/// The most entries one [`Command::Rows`] may name: several screens of
/// rows. The core leaves out any past it.
pub const MAX_ROWS: u32 = 512;

/// A request from a client to the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Ask the core to answer with [`Event::Pong`], to show it is running.
    Ping,
    /// List the folder at `path` in `order`. The answer is
    /// [`Event::FolderListed`] or [`Event::FolderUnreadable`] with the same
    /// `id`. A newer `ListFolder` from the same client cancels this one,
    /// which then may never answer. An order by modified time or size reads
    /// every entry's details before the listing goes out. Hidden entries
    /// are listed only while the [`crate::HiddenFiles`] setting shows them.
    ListFolder {
        id: ListingId,
        path: PathBuf,
        order: Order,
    },
    /// The entries of listing `id` the client shows or is about to show:
    /// `count` entries from `first`. The core reads the details of those it
    /// hasn't read yet, answering with [`Event::DetailsRead`], and the
    /// thumbnails feature makes their thumbnails at `size`.
    /// Each request replaces the one before, so entries scrolled away are
    /// dropped; a new `size` sends every thumbnail again. Entries never
    /// asked for never have their details read. At most [`MAX_ROWS`]
    /// entries count.
    Rows {
        id: ListingId,
        first: EntryId,
        count: u32,
        size: ThumbnailSize,
    },
    /// Ask for the UI's window state; the answer is [`Event::WindowState`].
    WindowState,
    /// Keep these bytes as the UI's window state, between runs too. The
    /// core never reads them; the UI owns what they say. Bytes over the
    /// core's cap (64 KiB) are dropped.
    SaveWindowState(Vec<u8>),
    /// Hear about every job from now on: first an [`Event::JobRunning`]
    /// for each job already running or paused and an
    /// [`Event::JobInterrupted`] for each a crash cut short, then each
    /// job's news. A
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
    /// Let a paused job go on, or start an interrupted one again where it
    /// left off. Every client hears [`Event::JobResumed`].
    ResumeJob(JobId),
    /// Drop an interrupted job: it isn't resumed, and what it finished
    /// before the crash stays. Every client hears it end as
    /// [`Outcome::Cancelled`].
    ForgetJob(JobId),
    /// Stop job `job` before its next change to the disk, paused or not.
    /// It ends with [`Outcome::Cancelled`].
    CancelJob(JobId),
    /// Answer `conflict`, the one job `job` holds at. An answer to a
    /// conflict no longer open (another window answered it first), or one
    /// its kind doesn't accept, is ignored. Every client hears
    /// [`Event::ConflictAnswered`].
    AnswerConflict {
        job: JobId,
        conflict: Conflict,
        answer: Answer,
        scope: Scope,
    },
    /// Answer `conflict`, a [`ConflictKind::PasswordNeeded`] or
    /// [`ConflictKind::PasswordWrong`] that job `job` holds at, with
    /// `password`: the job tries again with it. The password is never
    /// told to other clients or written to disk; every client hears
    /// [`Event::ConflictAnswered`] with [`Answer::Retry`].
    ///
    /// [`ConflictKind::PasswordNeeded`]: crate::ConflictKind::PasswordNeeded
    /// [`ConflictKind::PasswordWrong`]: crate::ConflictKind::PasswordWrong
    GivePassword {
        job: JobId,
        conflict: Conflict,
        password: String,
    },
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
    /// in entry order, answering [`Command::Rows`], and each entry's icon
    /// in the same order. Each entry's details come at most once per
    /// listing.
    DetailsRead {
        id: ListingId,
        first: EntryId,
        details: Arc<[Details]>,
        icons: Arc<[FileIcon]>,
    },
    /// How many bytes the folders of listing `id` hold so far, by entry,
    /// sent after a listing by [`crate::SortKey::Size`] while
    /// [`crate::FolderSizes::Measured`], a few times a second for the
    /// folders whose totals changed. Once every folder is measured the
    /// listing comes again under the same `id`, sorted by those totals,
    /// with each folder's total in its [`Details`], followed by one more of
    /// these with every folder's total by its place in that listing. Rows,
    /// details and thumbnails asked for before the sorted listing do not
    /// arrive after it.
    FoldersMeasured {
        id: ListingId,
        sizes: Vec<(EntryId, u64)>,
    },
    /// The UI's window state as last saved, empty when there is none,
    /// answering [`Command::WindowState`].
    WindowState(Vec<u8>),
    /// [`Command::StartJob`] (or a drop) `tag` runs as job `job`, doing
    /// `operation`. Only the client that asked hears it, and
    /// before anything else about the job.
    JobStarted {
        tag: JobTag,
        job: JobId,
        operation: Operation,
    },
    /// Job `job`, started before this client watched jobs, runs (or holds,
    /// when `paused`) doing `operation`, this far along. Answers
    /// [`Command::WatchJobs`].
    JobRunning {
        job: JobId,
        operation: Operation,
        progress: Progress,
        paused: bool,
        /// The conflict it holds at, if any.
        conflict: Option<Conflict>,
    },
    /// Job `job` holds at `conflict` until a client answers it. Every
    /// client that watches jobs hears it.
    JobConflict { job: JobId, conflict: Conflict },
    /// A client answered job `job`'s conflict, so it goes on. Every client
    /// that watches jobs hears it, so other windows close their question.
    ConflictAnswered { job: JobId, answer: Answer },
    /// How far job `job` has got, a few times a second while it runs.
    /// Every client hears it.
    JobProgress { job: JobId, progress: Progress },
    /// Job `job` holds until resumed or cancelled. Every client hears it.
    JobPaused(JobId),
    /// Job `job` goes on. Every client hears it.
    JobResumed(JobId),
    /// Job `job`, doing `operation`, was cut short when the core last
    /// stopped (a crash, or the power going). What it finished stays and
    /// nothing is left half-written; it waits for a client to resume or
    /// forget it. Answers [`Command::WatchJobs`].
    JobInterrupted { job: JobId, operation: Operation },
    /// Job `job` has ended. `changed` names the folders whose entries it
    /// changed, so a window showing one reads it again, and `tally` what
    /// the answers to its conflicts made of it. Every client that watches
    /// jobs hears it.
    JobFinished {
        job: JobId,
        outcome: Outcome,
        changed: Vec<PathBuf>,
        tally: Tally,
    },
    /// Job `job`, which did `operation`, can be undone with
    /// [`Operation::Undo`]. Every client that watches jobs hears it just
    /// before the job ends, and on [`Command::WatchJobs`] for each job that still
    /// can, oldest first. The core keeps the last 50 while it runs; they
    /// are lost when it exits.
    Undoable { job: JobId, operation: Operation },
    /// Job `job` can no longer be undone: it was, or it is too old.
    UndoGone(JobId),
}
