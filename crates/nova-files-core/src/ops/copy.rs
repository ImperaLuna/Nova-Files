//! The copy job: plan everything first, then make each item in order
//! (folders, files through part files, links as links), recording each in
//! the journal, and last give each folder made its source's mode, times
//! and attributes, deepest first, so a read-only folder can still be
//! filled. Before the job says it is done, every folder it added entries
//! to is synced, so what it reports made is on the disk.
//!
//! Whatever stands in an item's way (a name taken, a full drive, a
//! source gone) is a conflict: the job pauses on it until a client
//! answers. A folder skipped takes everything in it along; a folder kept
//! under a new name takes its contents there. Nothing is replaced unless
//! the answer says so, and then only through a backup or an exchange.
//! What was made before a failure or a cancel stays, whole.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{self, DirBuilder, File};
use std::io;
use std::os::unix::fs::{DirBuilderExt as _, symlink};
use std::path::{Path, PathBuf};

use nova_files_api::{Answer, Conflict, ConflictKind, Failure, Outcome};

use super::Ended;
use super::attrs::{copy_mode_and_times, copy_xattrs};
use super::clash::{clash, conflict_for};
use super::copy_file::{FileCopy, copy_file};
use super::journal::Record;
use super::keep_both::{Naming, free_path};
use super::place::{rename_no_replace, replace};
use super::plan::{Item, Kind, plan};
use super::running::Running;
use super::stop::Stop;

/// A folder being filled is the user's alone; it gets its source's mode
/// once full.
const FILLING_MODE: u32 = 0o700;

/// How many times a target may change hands between being looked at and
/// being made before the job gives up on it: another program keeps
/// changing it.
const LOOK_AGAIN: u32 = 3;

/// Makes an item at a target, replacing what is there when told to: see
/// [`make`]. A parameter, so tests can stand in for the disk.
type Make<'a> = dyn FnMut(&mut Running, &Item, &Path, usize, bool) -> Result<u64, Stop> + 'a;

/// What became of one planned item.
enum Placed {
    /// Made at its target; `left` of its bytes were holes, never copied.
    Made { left: u64 },
    /// Left out, with all it holds.
    Skipped,
    /// A folder already there that its contents go into.
    Merged,
}

/// Where the plan's items really go: items renamed to keep both, and
/// folders skipped with everything in them. Both are kept by source, as
/// two sources of the same name plan the same target and each gets its
/// own answer. Looking an item up costs as many lookups as its source is
/// deep, however many were answered.
#[derive(Default)]
struct Detours {
    /// Where each source renamed went.
    renamed: HashMap<PathBuf, PathBuf>,
    skipped: HashSet<PathBuf>,
}

impl Detours {
    /// Where `item` really goes, or `None` when it was skipped.
    fn target(&self, item: &Item) -> Option<PathBuf> {
        // The deepest renamed folder holding it, found first.
        let mut renamed = None;
        for above in item.from.ancestors() {
            if self.skipped.contains(above) {
                return None;
            }
            renamed = renamed.or_else(|| self.renamed.get(above).map(|to| (above, to)));
        }
        Some(renamed.map_or_else(
            || item.to.clone(),
            |(from, to)| to.join(item.from.strip_prefix(from).unwrap_or(&item.from)),
        ))
    }
}

/// Copies each of `sources` into `into`.
pub fn copy(running: &mut Running, sources: &[PathBuf], into: &Path) -> Ended {
    running.record(Record::Operation("copy"));
    running.record(Record::Into(into));
    for source in sources {
        running.record(Record::Source(source));
    }
    let plan = match plan(sources, into, &running.shared.control) {
        Ok(plan) => plan,
        Err(stop) => return (stop.outcome(), Vec::new()),
    };
    let items = u64::try_from(plan.items.len()).unwrap_or(u64::MAX);
    running.reporter.planned(plan.bytes, items);
    let mut detours = Detours::default();
    let mut folders = Vec::new();
    let mut touched = BTreeSet::new();
    let mut result = Ok(());
    for (index, item) in plan.items.iter().enumerate() {
        let Some(planned) = detours.target(item) else {
            running.reporter.item(size(item));
            continue;
        };
        let (placed, target) = match place(running, item, &planned, index, &mut make) {
            Ok(placed) => placed,
            Err(stop) => {
                result = Err(stop);
                break;
            }
        };
        let folder = item.kind == Kind::Folder;
        if target != planned {
            detours.renamed.insert(item.from.clone(), target.clone());
        }
        match placed {
            Placed::Made { left } => {
                if let Some(parent) = target.parent() {
                    touched.insert(parent.to_path_buf());
                }
                if folder {
                    folders.push((item.from.clone(), target));
                }
                running.reporter.item(left);
            }
            Placed::Skipped => {
                if folder {
                    detours.skipped.insert(item.from.clone());
                }
                running.reporter.item(size(item));
            }
            Placed::Merged => running.reporter.item(0),
        }
    }
    for (from, to) in folders.iter().rev() {
        // Held while paused; a cancel leaves the rest as they are.
        if let Err(cancelled) = running.shared.control.checkpoint() {
            result = Err(cancelled.into());
            break;
        }
        finish_folder(from, to);
    }
    // So what the job reports made is on the disk. A failure means only
    // that a crash right now could lose an entry, so it is logged.
    for folder in &touched {
        if let Err(error) = File::open(folder).and_then(|folder| folder.sync_all()) {
            log::warn!("can't sync the folder {}: {error}", folder.display());
        }
    }
    running.reporter.tell();
    let changed = if touched.is_empty() {
        Vec::new()
    } else {
        vec![into.to_path_buf()]
    };
    (
        result.map_or_else(Stop::outcome, |()| Outcome::Done),
        changed,
    )
}

/// The bytes `item` counts for in the job's progress.
const fn size(item: &Item) -> u64 {
    match item.kind {
        Kind::File { size } => size,
        Kind::Folder | Kind::Link | Kind::Special => 0,
    }
}

/// Puts `item`, the `index`th of its job, at `planned` or wherever the
/// user's answers to what stands in its way say, and returns what became
/// of it and where.
fn place(
    running: &mut Running,
    item: &Item,
    planned: &Path,
    index: usize,
    make: &mut Make<'_>,
) -> Result<(Placed, PathBuf), Stop> {
    let mut target = planned.to_path_buf();
    // The user said to replace the file at `target`; still looked at
    // before each try, as the name may have changed hands since.
    let mut replacing = false;
    let mut looked_again = 0;
    loop {
        running.shared.control.checkpoint()?;
        let tried = match clash(&item.from, item.kind, &target) {
            Err(error) => Err(Stop::at(&target, &error)),
            Ok(None) => make(running, item, &target, index, false),
            Ok(Some(ConflictKind::FileExists)) if replacing => {
                make(running, item, &target, index, true)
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
            Ok(left) => return Ok((Placed::Made { left }, target)),
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
                // Skip, or Retry: the only answers these kinds take.
                if running.ask(conflict(kind, item, &target))? == Answer::Skip {
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

/// Makes `item`, the `index`th of its job, at `target`, in the place of
/// what is there when `replacing`. Returns how many of its bytes were
/// holes, never copied.
fn make(
    running: &mut Running,
    item: &Item,
    target: &Path,
    index: usize,
    replacing: bool,
) -> Result<u64, Stop> {
    let job = running.job.0;
    let hidden = |what: &str| target.with_file_name(format!(".nova-{what}-{job}-{index}"));
    let (part, backup) = (hidden("part"), hidden("backup"));
    let mut left = 0;
    match item.kind {
        Kind::Folder => DirBuilder::new()
            .mode(FILLING_MODE)
            .create(target)
            .map_err(|error| Stop::at(target, &error))?,
        Kind::File { size } => {
            running.record(Record::Part(&part));
            if replacing {
                running.record(Record::Backup(&backup, target));
            }
            let mut copied = 0;
            let reporter = &mut running.reporter;
            let paths = FileCopy {
                from: &item.from,
                to: target,
                part: &part,
            };
            let copying = copy_file(
                &paths,
                &running.shared.control,
                &mut |bytes| {
                    copied += bytes;
                    reporter.bytes(bytes);
                },
                |part, to| put(part, to, &backup, replacing),
            );
            if let Err(stop) = copying {
                // The item is skipped or tried again from the start.
                running.reporter.take_back(copied);
                return Err(stop);
            }
            left = size.saturating_sub(copied);
        }
        Kind::Link => {
            let points_to =
                fs::read_link(&item.from).map_err(|error| Stop::at(&item.from, &error))?;
            if replacing {
                running.record(Record::Part(&part));
                running.record(Record::Backup(&backup, target));
            }
            let made = if replacing { &part } else { target };
            symlink(points_to, made).map_err(|error| Stop::at(target, &error))?;
            if replacing && let Err(error) = put(&part, target, &backup, replacing) {
                fs::remove_file(&part).ok();
                return Err(Stop::at(target, &error));
            }
        }
        Kind::Special => return Err(Stop::because(&item.from, Failure::Unsupported)),
    }
    running.record(Record::Made(target));
    Ok(left)
}

/// Gives the whole part file `part` the name `to`, replacing what has it
/// only when `replacing`.
fn put(part: &Path, to: &Path, backup: &Path, replacing: bool) -> io::Result<()> {
    if replacing {
        replace(part, to, backup)
    } else {
        rename_no_replace(part, to)
    }
}

/// Gives the folder made at `to` the attributes, mode and times of its
/// source `from`. The copy is whole without them, so a failure is only
/// logged.
fn finish_folder(from: &Path, to: &Path) {
    let finished = File::open(from).and_then(|source| {
        let target = File::open(to)?;
        copy_xattrs(&source, &target)?;
        copy_mode_and_times(&source.metadata()?, &target)
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
