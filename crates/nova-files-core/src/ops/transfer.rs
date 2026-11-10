//! Copy and move jobs: plan everything first, then make each item in
//! order (see `make`), recording each in the journal, and last give each
//! folder made its source's mode, times and attributes, deepest first, so
//! a read-only folder can still be filled. Before the job says it is
//! done, every folder it added entries to is synced, so what it reports
//! made is on the disk.
//!
//! A move first tries renaming each source whole, which on one drive is
//! all it takes, and plans only what that can't place. A folder it has to
//! make anew (across drives, or merged into one there) is removed from
//! the source once empty, after all it held has moved.
//!
//! Whatever stands in an item's way (a name taken, a full drive, a
//! source gone) is a conflict: the job pauses on it until a client
//! answers. A folder skipped takes everything in it along; a folder kept
//! under a new name takes its contents there. Nothing is replaced unless
//! the answer says so, and then only through a backup or an exchange.
//! What was made before a failure or a cancel stays, whole.

use std::cmp::Reverse;
use std::collections::{BTreeSet, HashSet};
use std::fs::{self, File, Metadata};
use std::io;
use std::path::{Path, PathBuf};

use nova_files_api::{Answer, Conflict, ConflictKind, Failure, Outcome};

use super::Ended;
use super::attrs::{copy_mode_and_times, copy_xattrs};
use super::clash::{clash, conflict_for};
use super::control::{Cancelled, Control};
use super::failure::failure;
use super::journal::Record;
use super::keep_both::{Naming, free_path};
use super::make::{How, Made, Making, make, size};
use super::place::rename_no_replace;
use super::plan::{Item, Kind, Plan, check, plan};
use super::running::Running;
use super::stop::Stop;

/// How many times a target may change hands between being looked at and
/// being made before the job gives up on it: another program keeps
/// changing it.
const LOOK_AGAIN: u32 = 3;

/// Makes an item at its target: see [`make`]. A parameter, so tests can
/// stand in for the disk.
type Make<'a> = dyn FnMut(&mut Running, &Making<'_>) -> Result<Made, Stop> + 'a;

/// What became of one planned item.
#[derive(Clone, Copy)]
enum Placed {
    /// Made at its target.
    Made(Made),
    /// Left out, with all it holds.
    Skipped,
    /// A folder already there that its contents go into.
    Merged,
}

/// Where the plan's paths really go: folders renamed to keep both, and
/// the folder settled with everything in it (skipped, or moved whole),
/// within one source's tree: two sources may plan the same target
/// (`/a/photos` and `/b/photos`), and each gets its own answers. The plan
/// lists a folder's contents right after it, so once a path is outside a
/// detour, no later one is inside: each is dropped then, and a lookup
/// costs only the folders it is in.
#[derive(Default)]
struct Detours {
    /// Innermost last.
    renamed: Vec<(PathBuf, PathBuf)>,
    settled: Option<PathBuf>,
}

impl Detours {
    /// Where `planned` really goes, or `None` when it needs nothing more.
    fn target(&mut self, planned: &Path) -> Option<PathBuf> {
        if let Some(settled) = &self.settled {
            if planned.starts_with(settled) {
                return None;
            }
            self.settled = None;
        }
        while let Some((from, _)) = self.renamed.last()
            && !planned.starts_with(from)
        {
            self.renamed.pop();
        }
        Some(self.renamed.last().map_or_else(
            || planned.to_path_buf(),
            |(from, to)| to.join(planned.strip_prefix(from).unwrap_or(planned)),
        ))
    }
}

/// What a transfer has done so far that is still to be finished.
#[derive(Default)]
struct Done {
    how: How,
    detours: Detours,
    /// Folders made, and their sources, in the order made.
    /// With the source's mode and times as they were then: a move changes
    /// them as it takes things out.
    folders: Vec<(PathBuf, PathBuf, io::Result<Metadata>)>,
    /// Source folders a move took everything out of, in the order met.
    emptied: Vec<PathBuf>,
    /// Folders whose entries changed.
    touched: BTreeSet<PathBuf>,
}

impl Done {
    /// `item`, planned for `planned`, was `placed` at `target`.
    fn note(
        &mut self,
        running: &mut Running,
        item: &Item,
        planned: &Path,
        (placed, target): (Placed, PathBuf),
    ) {
        let folder = item.kind == Kind::Folder;
        let moving = self.how.moves();
        // Only a folder takes others along, but a file's entry harms
        // nothing: the next item drops it.
        if target != planned {
            self.detours.renamed.push((item.to.clone(), target.clone()));
        }
        match placed {
            Placed::Made(made) => {
                let left = moving.then(|| item.from.parent()).flatten();
                for parent in [target.parent(), left].into_iter().flatten() {
                    self.touched.insert(parent.to_path_buf());
                }
                if made.carried {
                    self.detours.settled = Some(item.to.clone());
                } else if folder {
                    let times = item.from.symlink_metadata();
                    self.folders.push((item.from.clone(), target, times));
                    if moving {
                        self.emptied.push(item.from.clone());
                    }
                }
                running.reporter.item(made.left);
            }
            Placed::Skipped => {
                if folder {
                    self.detours.settled = Some(item.to.clone());
                }
                running.reporter.item(size(item));
            }
            Placed::Merged => {
                if moving {
                    self.emptied.push(item.from.clone());
                }
                running.reporter.item(0);
            }
        }
    }
}

/// Copies or moves, as `how` says, each of `sources` into `into`.
pub fn transfer(running: &mut Running, sources: &[PathBuf], into: &Path, how: How) -> Ended {
    running.record(Record::Operation(match how {
        How::Copy => "copy",
        How::Move => "move",
    }));
    running.record(Record::Into(into));
    for source in sources {
        running.record(Record::Source(source));
    }
    let mut touched = BTreeSet::new();
    let (rest, renamed) = if how.moves() {
        match move_whole(running, sources, into, &mut touched) {
            Ok(left) => left,
            Err(stop) => return (stop.outcome(), changed(into, sources, how, &touched)),
        }
    } else {
        (sources.to_vec(), 0)
    };
    let plan = match plan(&rest, into, &running.shared.control) {
        Ok(plan) => plan,
        Err(stop) => return (stop.outcome(), changed(into, sources, how, &touched)),
    };
    let items = u64::try_from(plan.items.len() + renamed).unwrap_or(u64::MAX);
    running.reporter.planned(plan.bytes, items);
    for _ in 0..renamed {
        running.reporter.item(0);
    }
    let mut done = Done {
        how,
        touched,
        ..Done::default()
    };
    let mut result = place_each(running, &plan, into, &mut done);
    let Done {
        folders,
        emptied,
        mut touched,
        ..
    } = done;
    for (from, to, times) in folders.into_iter().rev() {
        // Held while paused; a cancel leaves the rest as they are.
        if let Err(cancelled) = running.shared.control.checkpoint() {
            result = Err(cancelled.into());
            break;
        }
        finish_folder(&from, &to, times);
    }
    for folder in emptied.iter().rev() {
        // A cancel leaves the user's folders as they are.
        if let Err(stop) = remove_if_empty(running, folder, &mut touched) {
            result = result.and(Err(stop));
            break;
        }
    }
    // So what the job reports made is on the disk. A failure means only
    // that a crash right now could lose an entry, so it is logged.
    for folder in &touched {
        if let Err(error) = File::open(folder).and_then(|folder| folder.sync_all()) {
            log::warn!("can't sync the folder {}: {error}", folder.display());
        }
    }
    running.reporter.tell();
    (
        result.map_or_else(Stop::outcome, |()| Outcome::Done),
        changed(into, sources, how, &touched),
    )
}

/// Checks every one of a move's `sources` first, then renames each into
/// `into` whole where it can. Returns those it couldn't, and how many it
/// did; a source inside another moves with it.
///
/// # Errors
/// [`Stop`] when cancelled, or a source is refused.
fn move_whole(
    running: &mut Running,
    sources: &[PathBuf],
    into: &Path,
    touched: &mut BTreeSet<PathBuf>,
) -> Result<(Vec<PathBuf>, usize), Stop> {
    check(sources, into)?;
    let mut outermost = outermost(sources, &running.shared.control)?;
    // Deepest first: a source reached through a link among the sources
    // goes before that link's rename takes its path away.
    outermost.sort_by_key(|source| Reverse(source.components().count()));
    let rest = rename_whole(running, &outermost, into, touched)?;
    let renamed = outermost.len() - rest.len();
    Ok((rest, renamed))
}

/// The `sources` no other source's rename takes along: those not inside
/// another without passing through a link on the way. The real path of
/// the folder holding a source has no links in it, so only a real folder
/// among the sources can be one of its ancestors.
///
/// # Errors
/// [`Cancelled`] when cancelled.
fn outermost(sources: &[PathBuf], control: &Control) -> Result<Vec<PathBuf>, Cancelled> {
    let mut above = Vec::with_capacity(sources.len());
    for source in sources {
        control.checkpoint()?;
        above.push(
            source
                .parent()
                .and_then(|parent| parent.canonicalize().ok()),
        );
    }
    let real: HashSet<PathBuf> = sources
        .iter()
        .zip(&above)
        .filter_map(|(source, above)| Some(above.as_ref()?.join(source.file_name()?)))
        .collect();
    Ok(sources
        .iter()
        .zip(&above)
        .filter(|(_, above)| {
            !above
                .as_ref()
                .is_some_and(|above| above.ancestors().any(|folder| real.contains(folder)))
        })
        .map(|(source, _)| source.clone())
        .collect())
}

/// Renames each of `sources`, all checked, into `into` whole where its
/// name there is free and the drive is the same, and returns those it
/// couldn't.
///
/// # Errors
/// [`Stop`] when cancelled.
fn rename_whole(
    running: &mut Running,
    sources: &[PathBuf],
    into: &Path,
    touched: &mut BTreeSet<PathBuf>,
) -> Result<Vec<PathBuf>, Stop> {
    let mut rest = Vec::new();
    for source in sources {
        running.shared.control.checkpoint()?;
        let (Some(name), Some(parent)) = (source.file_name(), source.parent()) else {
            rest.push(source.clone());
            continue;
        };
        let target = into.join(name);
        // Anything at the target, even the source itself or a hard link
        // to it (which a rename would leave in place, saying it moved),
        // is the plan's to ask about.
        let free = target.symlink_metadata().is_err();
        if free && rename_no_replace(source, &target).is_ok() {
            running.record(Record::Moved(source, &target));
            touched.insert(into.to_path_buf());
            touched.insert(parent.to_path_buf());
        } else {
            rest.push(source.clone());
        }
    }
    Ok(rest)
}

/// Removes the source folder `folder` a move emptied. Anything still in
/// it (skipped, or added since) keeps it; any other failure is asked
/// about, as a file's would be.
///
/// # Errors
/// [`Stop`] when cancelled, or the failure can't be asked about.
fn remove_if_empty(
    running: &mut Running,
    folder: &Path,
    touched: &mut BTreeSet<PathBuf>,
) -> Result<(), Stop> {
    loop {
        running.shared.control.checkpoint()?;
        let error = match fs::remove_dir(folder) {
            Ok(()) => {
                if let Some(parent) = folder.parent() {
                    touched.insert(parent.to_path_buf());
                }
                return Ok(());
            }
            Err(error) => error,
        };
        let reason = failure(&error);
        if matches!(reason, Failure::NameTaken | Failure::NotFound) {
            log::info!("{} stays: {error}", folder.display());
            return Ok(());
        }
        let Some(kind) = conflict_for(reason, folder, folder) else {
            return Err(Stop::because(folder, reason));
        };
        let asked = Conflict {
            kind,
            source: folder.to_path_buf(),
            target: folder.to_path_buf(),
            // Numbered when asked.
            number: 0,
        };
        if running.skips(asked)? {
            return Ok(());
        }
    }
}

/// The folders whose entries a job changed: `into` once anything went
/// there, and, for a move, each source's folder.
fn changed(
    into: &Path,
    sources: &[PathBuf],
    how: How,
    touched: &BTreeSet<PathBuf>,
) -> Vec<PathBuf> {
    if touched.is_empty() {
        return Vec::new();
    }
    let mut changed = vec![into.to_path_buf()];
    if how.moves() {
        for parent in sources.iter().filter_map(|source| source.parent()) {
            if !changed.iter().any(|known| known == parent) {
                changed.push(parent.to_path_buf());
            }
        }
    }
    changed
}

/// Places each item of `plan`, a transfer into `into`, in turn, noting in
/// `done` what became of it.
fn place_each(
    running: &mut Running,
    plan: &Plan,
    into: &Path,
    done: &mut Done,
) -> Result<(), Stop> {
    for (index, item) in plan.items.iter().enumerate() {
        // A source's tree starts: the last one's answers were its own.
        if item.to.parent() == Some(into) {
            done.detours = Detours::default();
        }
        let Some(planned) = done.detours.target(&item.to) else {
            running.reporter.item(size(item));
            continue;
        };
        let making = Making {
            item,
            target: &planned,
            index,
            replacing: false,
            how: done.how,
        };
        let placed = place(running, &making, &mut make)?;
        done.note(running, item, &planned, placed);
    }
    Ok(())
}

/// Puts the item `planned` names at its target, or wherever the user's
/// answers to what stands in its way say, and returns what became of it
/// and where.
fn place(
    running: &mut Running,
    planned: &Making<'_>,
    make: &mut Make<'_>,
) -> Result<(Placed, PathBuf), Stop> {
    let item = planned.item;
    let mut target = planned.target.to_path_buf();
    // The user said to replace the file at `target`; still looked at
    // before each try, as the name may have changed hands since.
    let mut replacing = false;
    let mut looked_again = 0;
    loop {
        running.shared.control.checkpoint()?;
        let mut making = Making {
            target: &target,
            ..*planned
        };
        let tried = match clash(&item.from, item.kind, &target) {
            Err(error) => Err(Stop::at(&target, &error)),
            Ok(None) => make(running, &making),
            Ok(Some(ConflictKind::FileExists)) if replacing => {
                making.replacing = true;
                make(running, &making)
            }
            Ok(Some(kind)) => {
                replacing = false;
                match running.ask(conflict(kind, item, &target))? {
                    Answer::Skip => return Ok((Placed::Skipped, target)),
                    Answer::Merge => return Ok((Placed::Merged, target)),
                    Answer::Replace => replacing = true,
                    Answer::Retry => {}
                    Answer::KeepBoth => match free_path(&target, naming(item.kind)) {
                        Ok(free) => target = free,
                        Err(error) => return Err(Stop::at(&target, &error)),
                    },
                }
                continue;
            }
        };
        match tried {
            Ok(it) => return Ok((Placed::Made(it), target)),
            // The name changed hands since it was looked at: look again.
            Err(Stop::Failed { path, reason })
                if looked_again < LOOK_AGAIN
                    && path == target
                    && matches!(reason, Failure::NameTaken | Failure::NotFound) =>
            {
                looked_again += 1;
            }
            Err(Stop::Failed { path, reason }) => {
                let Some(kind) = conflict_for(reason, &path, &item.from) else {
                    return Err(Stop::Failed { path, reason });
                };
                if running.skips(conflict(kind, item, &target))? {
                    return Ok((Placed::Skipped, target));
                }
            }
            Err(stop) => return Err(stop),
        }
    }
}

fn conflict(kind: ConflictKind, item: &Item, target: &Path) -> Conflict {
    Conflict {
        kind,
        source: item.from.clone(),
        target: target.to_path_buf(),
        // Numbered when asked.
        number: 0,
    }
}

const fn naming(kind: Kind) -> Naming {
    match kind {
        Kind::Folder => Naming::Folder,
        Kind::File { .. } | Kind::Link | Kind::Special => Naming::File,
    }
}

/// Gives the folder made at `to` the attributes of its source `from`, and
/// the mode and times `times` read from it when the folder was made. The
/// copy is whole without them, so a failure is only logged.
fn finish_folder(from: &Path, to: &Path, times: io::Result<Metadata>) {
    let finished = File::open(from).and_then(|source| {
        let target = File::open(to)?;
        copy_xattrs(&source, &target)?;
        copy_mode_and_times(&times?, &target)
    });
    if let Err(error) = finished {
        log::warn!(
            "can't give {} its source's mode and times: {error}",
            to.display()
        );
    }
}

#[cfg(test)]
mod tests;
