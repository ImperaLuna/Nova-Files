//! What clients say to the core and what the core says back.

use std::path::PathBuf;
use std::sync::Arc;

use crate::drive::{DriveAction, DriveFailure, DriveList};
use crate::folder::{Details, EntryId, Listing, ListingId, Place, Unreadable};
use crate::job::{Answer, Conflict, JobId, JobTag, Operation, Outcome, Progress, Scope};
use crate::properties::{Properties, PropertiesId};
use crate::search::{Hit, SearchFailed, SearchId};
use crate::settings::{SettingChange, Settings};
use crate::size::{MeasureId, ScanState, Size};
use crate::theme::{DesktopLook, Theme};

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
    /// Read the properties of what is at `path`, without following a link.
    /// The answer is [`Event::Properties`] or [`Event::PropertiesUnreadable`]
    /// with the same `id`.
    Properties { id: PropertiesId, path: PathBuf },
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
    /// Ask for the settings; the answer is [`Event::Settings`], and from
    /// then on the client gets [`Event::Settings`] again each time one
    /// changes. Clients that never ask never get them.
    Settings,
    /// Change one setting and save it. Every client that asked for the
    /// settings, this one included, then gets [`Event::Settings`]; nothing
    /// is sent when the value was already set.
    ChangeSetting(SettingChange),
    /// Ask for the theme; the answer is [`Event::Theme`], and from then on
    /// the client gets [`Event::Theme`] again each time it changes. Clients
    /// that never ask never get it.
    Theme,
    /// Tell the core what the desktop asks apps to look like; sent by the
    /// D-Bus side each time the desktop changes it. Clients that asked for
    /// the theme get [`Event::Theme`] if the theme changed; nothing
    /// answers the sender.
    DesktopLook(DesktopLook),
    /// Ask for the drives and volumes; the answer is [`Event::Drives`],
    /// and from then on the client gets [`Event::Drives`] again each time
    /// one comes, goes or changes. Clients that never ask never get them.
    Drives,
    /// Mount, unmount or eject. The answer, once it is done, is
    /// [`Event::DriveDone`]; the change itself reaches every client that
    /// asked for the drives as [`Event::Drives`].
    Drive(DriveAction),
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
    /// Measure how much space everything under `paths` (absolute paths)
    /// takes, together: what two paths share counts once. The answer is
    /// [`Event::SizeMeasured`] with the same `id`, a few times a second
    /// while the scan runs, then once more when it ends. A client has one
    /// scan at a time: a newer `Measure` from it stops this one, which then
    /// says nothing more.
    Measure { id: MeasureId, paths: Vec<PathBuf> },
    /// Stop the client's scan `id`, if it still runs. It says nothing more.
    StopMeasure(MeasureId),
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
    /// The properties of [`Command::Properties`] `id`'s path.
    Properties {
        id: PropertiesId,
        properties: Arc<Properties>,
    },
    /// [`Command::Properties`] `id`'s path couldn't be looked at.
    PropertiesUnreadable {
        id: PropertiesId,
        path: PathBuf,
        reason: Unreadable,
    },
    /// [`Command::OpenFile`]'s app has started.
    FileOpened,
    /// [`Command::OpenFile`] started nothing: sandboxed, no default app for
    /// the file's type, or the app's program couldn't start.
    FileNotOpened,
    /// The UI's window state as last saved, empty when there is none,
    /// answering [`Command::WindowState`].
    WindowState(Vec<u8>),
    /// The settings, answering [`Command::Settings`], and sent again to
    /// each client that asked whenever one changes: from a client, or from
    /// the settings file.
    Settings(Arc<Settings>),
    /// The theme, answering [`Command::Theme`], and sent again to each
    /// client that asked whenever it changes: from the theme file, or the
    /// appearance setting.
    Theme(Arc<Theme>),
    /// The drives and volumes, answering [`Command::Drives`], and sent
    /// again to each client that asked whenever one comes, goes or
    /// changes.
    Drives(Arc<DriveList>),
    /// [`Command::Drive`] `action` is done: for a mount, with where the
    /// volume is now. Only the client that asked hears it.
    DriveDone {
        action: DriveAction,
        result: Result<Option<PathBuf>, DriveFailure>,
    },
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
    /// changed, so a window showing one reads it again. Every client that
    /// watches jobs hears it.
    JobFinished {
        job: JobId,
        outcome: Outcome,
        changed: Vec<PathBuf>,
    },
    /// What [`Command::Measure`] `id` counted so far, and whether it ended.
    SizeMeasured {
        id: MeasureId,
        size: Size,
        state: ScanState,
    },
}

/// Names one connected client, so the core can answer the right one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClientId(pub u64);
