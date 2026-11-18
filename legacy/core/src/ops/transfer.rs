use std::cmp::Reverse;
use std::collections::{BTreeSet, HashSet};
use std::fs::{File, Metadata};
use std::io;
use std::mem;
use std::path::{Path, PathBuf};

use nova_files_api::{Answer, Conflict, ConflictKind, Failure, Outcome};
use rustix::fs::{AtFlags, Mode, OFlags, openat, unlinkat};

use super::Ended;
use super::ahead::Ahead;
use super::attrs::{copy_mode_and_times, copy_xattrs};
use super::clash::{clash, conflict_for};
use super::control::{Cancelled, Control};
use super::failure::failure;
use super::folders::{Folders, Unreached};
use super::journal::{Record, word};
use super::keep_both::{Naming, free_path};
use super::make::{How, Made, Making, make};
use super::place::rename_no_replace;
use super::plan::{Item, Kind, Plan, check, check_pairs, pairs_into, plan_pairs};
use super::running::Running;
use super::stop::Stop;
use super::sync::sync_folders;

const LOOK_AGAIN: u32 = 3;

type Make<'a> = dyn FnMut(&mut Running, &Making<'_>) -> Result<Made, Stop> + 'a;

#[derive(Clone, Copy)]
enum Placed {
    Made(Made),
    Skipped,
    Merged,
}

#[derive(Default)]
struct Detours {
    renamed: Vec<(PathBuf, PathBuf)>,
    settled: Option<PathBuf>,
}

impl Detours {
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

#[derive(Default)]
struct Done {
    how: How,
    detours: Detours,
    folders: Vec<(PathBuf, PathBuf, io::Result<Metadata>)>,
    emptied: Vec<PathBuf>,
    touched: BTreeSet<PathBuf>,
}

impl Done {
    fn note(
        &mut self,
        running: &mut Running,
        item: &Item,
        planned: &Path,
        (placed, target): (Placed, PathBuf),
    ) {
        let folder = item.kind == Kind::Folder;
        let moving = self.how.moves();
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
                    let times = look_at_source(&mut running.folders, &item.from);
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
                running.reporter.item(item.size());
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

pub fn transfer(running: &mut Running, sources: &[PathBuf], into: &Path, how: How) -> Ended {
    let name = match how {
        How::Copy => word::COPY,
        How::Move => word::MOVE,
    };
    running.record_job(name, Some(into), sources);
    if let Err(stop) = check(sources, into) {
        return (stop.outcome(), Vec::new());
    }
    let (result, touched) = transfer_to(running, &pairs_into(sources, into), how);
    (
        result.map_or_else(Stop::outcome, |()| Outcome::Done),
        changed(into, sources, how, &touched),
    )
}

pub fn transfer_to(
    running: &mut Running,
    pairs: &[(PathBuf, PathBuf)],
    how: How,
) -> (Result<(), Stop>, BTreeSet<PathBuf>) {
    let mut touched = BTreeSet::new();
    let (rest, renamed) = if how.moves() {
        match move_whole(running, pairs, &mut touched) {
            Ok(left) => left,
            Err(stop) => return (Err(stop), touched),
        }
    } else {
        (pairs.to_vec(), 0)
    };
    let plan = match plan_pairs(&rest, &running.shared.control) {
        Ok(plan) => plan,
        Err(stop) => return (Err(stop), touched),
    };
    running.folders = Folders::of(&plan);
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
    let mut result = place_each(running, &plan, &rest, &mut done);
    let mut sources = mem::take(&mut running.folders);
    let Done {
        folders,
        emptied,
        mut touched,
        ..
    } = done;
    for (from, to, times) in folders.into_iter().rev() {
        if let Err(cancelled) = running.shared.control.checkpoint() {
            result = Err(cancelled.into());
            break;
        }
        finish_folder(&mut sources, &from, &to, times);
    }
    for folder in emptied.iter().rev() {
        if let Err(stop) = remove_if_empty(running, &mut sources, folder, &mut touched) {
            result = result.and(Err(stop));
            break;
        }
    }
    sync_folders(touched.iter().map(PathBuf::as_path));
    running.reporter.tell();
    (result, touched)
}

fn move_whole(
    running: &mut Running,
    pairs: &[(PathBuf, PathBuf)],
    touched: &mut BTreeSet<PathBuf>,
) -> Result<(Vec<(PathBuf, PathBuf)>, usize), Stop> {
    check_pairs(pairs)?;
    let sources: Vec<_> = pairs.iter().map(|(source, _)| source.clone()).collect();
    let outermost: HashSet<_> = outermost(&sources, &running.shared.control)?
        .into_iter()
        .collect();
    let mut outermost: Vec<_> = pairs
        .iter()
        .filter(|(source, _)| outermost.contains(source))
        .cloned()
        .collect();
    outermost.sort_by_key(|(source, _)| Reverse(source.components().count()));
    let rest = rename_whole(running, &outermost, touched)?;
    let renamed = outermost.len() - rest.len();
    Ok((rest, renamed))
}

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

fn rename_whole(
    running: &mut Running,
    pairs: &[(PathBuf, PathBuf)],
    touched: &mut BTreeSet<PathBuf>,
) -> Result<Vec<(PathBuf, PathBuf)>, Stop> {
    let mut rest = Vec::new();
    for (source, target) in pairs {
        running.shared.control.checkpoint()?;
        let (Some(into), Some(parent)) = (target.parent(), source.parent()) else {
            rest.push((source.clone(), target.clone()));
            continue;
        };
        let free = target.symlink_metadata().is_err();
        if free && rename_no_replace(source, target).is_ok() {
            running.record(Record::Moved(source, target));
            touched.insert(into.to_path_buf());
            touched.insert(parent.to_path_buf());
        } else {
            rest.push((source.clone(), target.clone()));
        }
    }
    Ok(rest)
}

fn remove_if_empty(
    running: &mut Running,
    sources: &mut Folders,
    folder: &Path,
    touched: &mut BTreeSet<PathBuf>,
) -> Result<(), Stop> {
    let removed = running.retry_or_skip(folder, || {
        let removed = sources.planned_folder(folder).and_then(|source| {
            let at = source.at();
            Ok(unlinkat(at.folder, at.name, AtFlags::REMOVEDIR)?)
        });
        match removed {
            Ok(()) => return Ok(true),
            Err(Unreached::Changed) => {}
            Err(Unreached::Failed(errno)) => {
                let reason = failure(&errno.into());
                if !matches!(reason, Failure::NameTaken | Failure::NotFound) {
                    return Err(reason);
                }
            }
        }
        let why = removed.err().map(|why| why.to_string()).unwrap_or_default();
        log::info!("{} stays: {why}", folder.display());
        Ok(false)
    })?;
    if removed == Some(true)
        && let Some(parent) = folder.parent()
    {
        touched.insert(parent.to_path_buf());
    }
    Ok(())
}

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

fn place_each(
    running: &mut Running,
    plan: &Plan,
    pairs: &[(PathBuf, PathBuf)],
    done: &mut Done,
) -> Result<(), Stop> {
    let tops: HashSet<(&Path, &Path)> = pairs
        .iter()
        .map(|(source, target)| (source.as_path(), target.as_path()))
        .collect();
    let mut ahead = Ahead::new(&plan.items);
    for (index, item) in plan.items.iter().enumerate() {
        let top = tops.contains(&(item.from.as_path(), item.to.as_path()));
        if top {
            done.detours = Detours::default();
        }
        let Some(planned) = done.detours.target(&item.to) else {
            running.reporter.item(item.size());
            continue;
        };
        let making = Making {
            item,
            target: &planned,
            index,
            replacing: false,
            how: done.how,
        };
        let placed = place(running, &making, &mut make, &mut ahead)?;
        if top && matches!(placed.0, Placed::Skipped) {
            running.tally.skipped += 1;
        }
        done.note(running, item, &planned, placed);
    }
    Ok(())
}

fn place(
    running: &mut Running,
    planned: &Making<'_>,
    make: &mut Make<'_>,
    ahead: &mut Ahead<'_>,
) -> Result<(Placed, PathBuf), Stop> {
    let item = planned.item;
    let mut target = planned.target.to_path_buf();
    let mut replacing = false;
    let mut looked_again = 0;
    running.shared.control.checkpoint()?;
    if let Some(made) = running
        .made_before(&item.from, item.kind)
        .filter(|made| made.parent() == target.parent())
    {
        let made_before = Made {
            left: item.size(),
            carried: false,
        };
        return Ok((Placed::Made(made_before), made.to_path_buf()));
    }
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
                let mut asked = conflict(kind, item, &target);
                if !running.answered(kind) {
                    asked.more = ahead.more(planned.index, kind, &running.shared.control);
                }
                match running.ask(asked)? {
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
            Ok(it) => {
                if making.replacing {
                    running.tally.replaced += 1;
                }
                return Ok((Placed::Made(it), target));
            }
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
        number: 0,
        more: 0,
    }
}

const fn naming(kind: Kind) -> Naming {
    match kind {
        Kind::Folder => Naming::Folder,
        Kind::File { .. } | Kind::Link | Kind::Special => Naming::File,
    }
}

fn look_at_source(sources: &mut Folders, path: &Path) -> io::Result<Metadata> {
    sources.source(path)?.at().look()
}

fn finish_folder(sources: &mut Folders, from: &Path, to: &Path, times: io::Result<Metadata>) {
    let flags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::CLOEXEC);
    let opened = sources
        .open(from)
        .and_then(|folder| Ok(openat(folder, ".", flags, Mode::empty())?));
    let finished = opened.map_err(io::Error::from).and_then(|source| {
        let source = File::from(source);
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
