//! What clients say to the core and what the core says back.

use std::path::PathBuf;
use std::sync::Arc;

use crate::bookmark::{Bookmark, BookmarkChange};
use crate::disk_space::PathSpace;
use crate::drive::{DriveAction, DriveFailure, DriveList};
use crate::folder::{Details, EntryId, Listing, ListingId, Order, Place, Unreadable};
use crate::job::{Answer, Conflict, JobId, JobTag, Operation, Outcome, Progress, Scope};
use crate::open_with::{OpenWith, OpenWithAnswer};
use crate::preview::{Fit, Preview, PreviewId};
use crate::properties::{Properties, PropertiesId};
use crate::recent::{RecentFolder, RecentId};
use crate::search::{Hit, SearchFailed, SearchId};
use crate::settings::{SettingChange, Settings};
use crate::size::{MeasureId, ScanState, Share, Size};
use crate::theme::{DesktopLook, Theme};
use crate::trash::Trashed;

/// The most entries one [`Command::Rows`] may name: several screens of
/// rows. The core leaves out any past it.
pub const MAX_ROWS: u32 = 512;

/// The most hits one search stage may hold: more than a launcher shows.
/// The core keeps the best ones.
pub const MAX_HITS: u32 = 200;

/// The most entries each stage of a [`Command::Find`] adds: more than a
/// window's list scrolls through when looking for one name.
pub const MAX_FOUND: u32 = 1000;

/// A request from a client to the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Ask the core to answer with [`Event::Pong`], to show it is running.
    Ping,
    /// List the folder at `path` in `order`. The answer is
    /// [`Event::FolderListed`] or [`Event::FolderUnreadable`] with the same
    /// `id`. A newer `ListFolder` from the same client cancels this one,
    /// which then may never answer. An order by modified time or size reads
    /// every entry's details before the listing goes out.
    ListFolder {
        id: ListingId,
        path: PathBuf,
        order: Order,
    },
    /// Ask for the places list: the answer is [`Event::Places`] and
    /// [`Event::Bookmarks`], and from then on the client gets each again
    /// whenever it changes (a folder made or removed, the bookmarks file
    /// edited). Clients that never ask never get them.
    Places,
    /// Pin, unpin, rename or move a pinned folder and save it. Every client
    /// that asked for the places, this one included, then gets
    /// [`Event::Bookmarks`]; nothing is sent when nothing changed.
    ChangeBookmark(BookmarkChange),
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
    /// Find files whose names match `query`, as [`Command::Search`] does,
    /// for a window to show as its list. The answer is
    /// [`Event::FolderListed`] with `id` and every stage's hits, best
    /// first, each entry in its own folder (see [`Listing::folders`]), at
    /// most [`MAX_FOUND`] a stage; or [`Event::FindFailed`]. `within` keeps
    /// only what is inside that folder, at any depth; without it the whole
    /// index counts. It is the client's listing like a
    /// [`Command::ListFolder`]: either one asked for later cancels it, and
    /// [`Command::Rows`] serves its rows.
    Find {
        id: ListingId,
        query: String,
        within: Option<PathBuf>,
    },
    /// Read the properties of what is at `path`, without following a link.
    /// The answer is [`Event::Properties`] or [`Event::PropertiesUnreadable`]
    /// with the same `id`.
    Properties { id: PropertiesId, path: PathBuf },
    /// Look into what is at `path`, following a link, for a quick look; a
    /// picture comes scaled down to `fit`. The answer is
    /// [`Event::Previewed`] or [`Event::PreviewUnreadable`] with the same
    /// `id`. A newer `Preview` from the same client cancels this one, which
    /// then may never answer.
    Preview {
        id: PreviewId,
        path: PathBuf,
        fit: Fit,
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
    /// List the apps that open some files, open them in one, or make one
    /// the default (see [`OpenWith`]).
    OpenWith(OpenWith),
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
    /// Ask what is in the desktop's trash: in the home trash and in each
    /// mounted drive's. The answer is [`Event::Trash`].
    ListTrash,
    /// Mount, unmount or eject. The answer, once it is done, is
    /// [`Event::DriveDone`]; the change itself reaches every client that
    /// asked for the drives as [`Event::Drives`].
    Drive(DriveAction),
    /// Ask how full the drives holding `paths` (absolute paths) are. The
    /// answer is [`Event::DiskSpace`], one entry per path in the same
    /// order; paths past [`crate::MAX_DISK_SPACE_PATHS`] are left out.
    DiskSpace(Vec<PathBuf>),
    /// A person opened `folder` (an absolute path); it goes first among the
    /// recent folders. Nothing answers.
    FolderUsed(PathBuf),
    /// Ask for the newest `count` recent folders that can still be read,
    /// leaving out those in `skip` (the ones a client already shows
    /// elsewhere). The answer is [`Event::RecentFolders`] with the same
    /// `id`.
    RecentFolders {
        id: RecentId,
        count: u32,
        skip: Vec<PathBuf>,
    },
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
    /// Copy or move `sources` into the folder `into`, as dragging them
    /// there does: moved when every one is on `into`'s drive, copied
    /// otherwise. The core picks; [`Event::JobStarted`] says which, and
    /// the job then runs as a [`Command::StartJob`] of it would.
    DropFiles {
        tag: JobTag,
        sources: Vec<PathBuf>,
        into: PathBuf,
    },
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
    /// Measure what each entry of `folder` (an absolute path) takes. The
    /// answer is [`Event::SpaceMeasured`] with the same `id`, a few times a
    /// second while the scan runs, then once more when it ends; or
    /// [`Event::SpaceUnreadable`]. A client has one space scan at a time,
    /// beside its one `Measure`: a newer `Space` from it stops this one,
    /// which then says nothing more.
    Space { id: MeasureId, folder: PathBuf },
    /// Stop the client's scan `id`, a `Measure` or a `Space`, if it still
    /// runs (both, when the client gave them the same id). It says nothing
    /// more.
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
    /// The pinned folders, in the user's order.
    Bookmarks(Arc<[Bookmark]>),
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
    /// [`Command::Find`] `id` couldn't run.
    FindFailed { id: ListingId, reason: SearchFailed },
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
    /// What [`Command::Preview`] `id` shows.
    Previewed {
        id: PreviewId,
        preview: Arc<Preview>,
    },
    /// [`Command::Preview`] `id`'s path couldn't be looked into.
    PreviewUnreadable {
        id: PreviewId,
        path: PathBuf,
        reason: Unreadable,
    },
    /// [`Command::OpenFile`]'s app has started.
    FileOpened,
    /// [`Command::OpenFile`] started nothing: sandboxed, no default app for
    /// the file's type, or the app's program couldn't start.
    FileNotOpened,
    /// The answer to a [`Command::OpenWith`] request.
    OpenWith(OpenWithAnswer),
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
    /// What is in the trash, answering [`Command::ListTrash`], in no
    /// particular order.
    Trash(Vec<Trashed>),
    /// [`Command::Drive`] `action` is done: for a mount, with where the
    /// volume is now. Only the client that asked hears it.
    DriveDone {
        action: DriveAction,
        result: Result<Option<PathBuf>, DriveFailure>,
    },
    /// How full the drives are, answering [`Command::DiskSpace`]: one
    /// entry per path asked, in the same order.
    DiskSpace(Vec<PathSpace>),
    /// The recent folders, answering [`Command::RecentFolders`] `id`,
    /// newest first.
    RecentFolders {
        id: RecentId,
        folders: Vec<RecentFolder>,
    },
    /// [`Command::StartJob`] or [`Command::DropFiles`] `tag` runs as job
    /// `job`, doing `operation`. Only the client that asked hears it, and
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
    /// changed, so a window showing one reads it again. Every client that
    /// watches jobs hears it.
    JobFinished {
        job: JobId,
        outcome: Outcome,
        changed: Vec<PathBuf>,
    },
    /// Job `job`, which did `operation`, can be undone with
    /// [`Operation::Undo`]. Every client that watches jobs hears it just
    /// before the job ends, and on [`Command::WatchJobs`] for each job that still
    /// can, oldest first. The core keeps the last 50 while it runs; they
    /// are lost when it exits.
    Undoable { job: JobId, operation: Operation },
    /// Job `job` can no longer be undone: it was, or it is too old.
    UndoGone(JobId),
    /// What [`Command::Measure`] `id` counted so far, and whether it ended.
    SizeMeasured {
        id: MeasureId,
        size: Size,
        state: ScanState,
    },
    /// What the entries of [`Command::Space`] `id`'s folder take so far:
    /// the first answer names every entry, later ones only those whose
    /// totals changed since. `state` says whether it ended.
    SpaceMeasured {
        id: MeasureId,
        shares: Vec<Share>,
        state: ScanState,
    },
    /// [`Command::Space`] `id`'s folder couldn't be listed.
    SpaceUnreadable { id: MeasureId, reason: Unreadable },
}

/// Names one connected client, so the core can answer the right one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClientId(pub u64);
