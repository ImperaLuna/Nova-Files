//! What clients say to the core and what the core says back.

use std::path::PathBuf;
use std::sync::Arc;

use crate::bookmark::{Bookmark, BookmarkChange};
use crate::disk_space::PathSpace;
use crate::drive::{DriveAction, DriveFailure, DriveList};
use crate::file_icon::FileIcon;
use crate::folder::{Details, EntryId, Listing, ListingId, Order, Place, Unreadable};
use crate::folder_view::{FolderView, FolderViews};
use crate::gathered::{Gathered, GatheredChange, Gathering};
use crate::job::{
    Answer, Conflict, DropAskId, DropKind, JobId, JobTag, Operation, Outcome, Progress, Scope,
    Tally,
};
use crate::preview::{Fit, Preview, PreviewId};
use crate::properties::{Properties, PropertiesId};
use crate::recent::{RecentFolder, RecentId};
use crate::search::{Hit, SearchFailed, SearchId};
use crate::settings::{SettingChange, Settings};
use crate::size::{MeasureId, ScanState, Share, Size};
use crate::theme::{DesktopLook, Theme};
use crate::thumbnail_size::ThumbnailSize;

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
    /// every entry's details before the listing goes out. Hidden entries
    /// are listed only while [`crate::Settings::hidden_files`] shows them.
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
    /// Ask for the folders Games and Projects gather from: the answer is
    /// [`Event::Gathered`], and from then on the client gets it again
    /// whenever they change (from any client, or `gathered.toml` edited).
    Gathered,
    /// Add or remove a gathered folder and save it. Every client that
    /// asked, this one included, then gets [`Event::Gathered`]; nothing is
    /// sent when nothing changed.
    ChangeGathered(GatheredChange),
    /// List what `place` gathers in `order`: the folders inside each of
    /// its folders that holds many, and each one-project folder itself,
    /// folders only, as the hidden-files setting says. The answer is
    /// [`Event::FolderListed`] with `id`, each entry in its own folder
    /// (see [`Listing::folders`]) and an empty [`Listing::path`]. It is
    /// the client's listing like a [`Command::ListFolder`]: either one
    /// asked for later cancels it, and [`Command::Rows`] serves its rows.
    ListGathered {
        id: ListingId,
        place: Gathering,
        order: Order,
    },
    /// The entries of listing `id` the client shows or is about to show:
    /// `count` entries from `first`. The core reads the details of those it
    /// hasn't read yet, answering with [`Event::DetailsRead`], and makes
    /// their thumbnails at `size`, answering each with [`Event::Thumbnail`].
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
    /// Ask for the folders' views: the answer is [`Event::FolderViews`],
    /// sent again whenever one changes.
    FolderViews,
    /// Show `folder` with `view` from now on, and save it. Every client
    /// that asked for the views, this one included, then gets
    /// [`Event::FolderViews`]. A path that isn't absolute is ignored.
    SetFolderView { folder: PathBuf, view: FolderView },
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
    /// Tell the core which icon themes the desktop names, in its order
    /// (none when it names none); sent by the D-Bus side each time they
    /// change. The core takes the first one installed when the toolkits'
    /// settings files name none; nothing answers.
    DesktopIconThemes(Vec<String>),
    /// Ask for the drives and volumes; the answer is [`Event::Drives`],
    /// and from then on the client gets [`Event::Drives`] again each time
    /// one comes, goes or changes. Clients that never ask never get them.
    Drives,
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
    /// Say what [`Command::DropFiles`] of `sources` into `into` would do,
    /// without doing it: for a drag to name its drop before it lands. The
    /// answer is [`Event::DropWould`] with the same `id`.
    DropWould {
        id: DropAskId,
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
    /// in entry order, answering [`Command::Rows`], and each entry's icon
    /// in the same order. Each entry's details come at most once per
    /// listing.
    DetailsRead {
        id: ListingId,
        first: EntryId,
        details: Arc<[Details]>,
        icons: Arc<[FileIcon]>,
    },
    /// A thumbnail for entry `entry` of listing `id`, as PNG bytes that fit
    /// in a square `size` across.
    Thumbnail {
        id: ListingId,
        entry: EntryId,
        size: ThumbnailSize,
        png: Arc<[u8]>,
    },
    /// The standard folders that exist, in places-list order.
    Places(Vec<Place>),
    /// The pinned folders, in the user's order.
    Bookmarks(Arc<[Bookmark]>),
    /// The folders Games and Projects gather from (see
    /// [`Command::Gathered`]).
    Gathered(Arc<Gathered>),
    /// What is inside `place`'s folders changed (a folder made, removed or
    /// renamed in one, or one of them appeared or went away): a client
    /// showing it lists it again. Sent to the clients that asked with
    /// [`Command::Gathered`].
    GatheredChanged(Gathering),
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
    /// The UI's window state as last saved, empty when there is none,
    /// answering [`Command::WindowState`].
    WindowState(Vec<u8>),
    /// Every folder given a view, newest first, answering
    /// [`Command::FolderViews`] and after each change.
    FolderViews(FolderViews),
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
    /// How full the drives are, answering [`Command::DiskSpace`]: one
    /// entry per path asked, in the same order.
    DiskSpace(Vec<PathSpace>),
    /// The recent folders, answering [`Command::RecentFolders`] `id`,
    /// newest first.
    RecentFolders {
        id: RecentId,
        folders: Vec<RecentFolder>,
    },
    /// [`Command::DropWould`] `id`'s drop would `kind` its files. Only the
    /// client that asked hears it.
    DropWould { id: DropAskId, kind: DropKind },
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
