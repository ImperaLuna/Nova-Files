//! The permanent delete job: plan everything first, then remove each item
//! last to first, so a folder goes after all it held. Links go, never
//! what they point to.
//!
//! Each item is removed from its folder, opened from the folder above it
//! without following links, so the delete never leaves the trees the user
//! chose, even if one changes while it runs.
//!
//! What the job may not remove is a conflict (skip it, or retry); a
//! folder that still holds something when its turn comes (what was
//! skipped, or what was added since the plan) stays, as nobody chose to
//! delete what is in it now.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

use nova_files_api::{Conflict, Outcome};
use rustix::fs::{AtFlags, Mode, OFlags, open, openat, unlinkat};
use rustix::io::Errno;

use super::Ended;
use super::clash::conflict_for;
use super::failure::failure;
use super::journal::{Record, word};
use super::make::size;
use super::plan::{Item, Kind, Plan, plan_delete};
use super::running::Running;
use super::stop::Stop;

/// Deletes each of `paths` for good.
pub fn delete(running: &mut Running, paths: &[PathBuf]) -> Ended {
    running.record(Record::Operation(word::DELETE));
    for path in paths {
        running.record(Record::Source(path));
    }
    // Shallowest first, so removing last to first takes a path chosen
    // through a chosen link before the link, whatever order they came in.
    let mut ordered = paths.to_vec();
    ordered.sort_by_key(|path| path.components().count());
    let plan = match plan_delete(&ordered, &running.shared.control)
        .and_then(|plan| planned(running, plan))
    {
        Ok(plan) => plan,
        Err(stop) => return (stop.outcome(), Vec::new()),
    };
    let items = u64::try_from(plan.items.len()).unwrap_or(u64::MAX);
    running.reporter.planned(plan.bytes, items);
    let mut changed = BTreeSet::new();
    // Only the chosen folders are walked; a chosen link is removed as a
    // link, and a path chosen through it is reached as the user named it.
    let chosen: HashSet<_> = paths.iter().collect();
    let trees: HashSet<_> = plan
        .items
        .iter()
        .filter(|item| item.kind == Kind::Folder && chosen.contains(&item.from))
        .map(|item| item.from.clone())
        .collect();
    let mut folders = Folders {
        trees: &trees,
        open: HashMap::new(),
    };
    let mut result = Ok(());
    for item in plan.items.iter().rev() {
        match remove(running, &mut folders, item) {
            Ok(removed) => {
                if removed && let Some(parent) = item.from.parent() {
                    changed.insert(parent.to_path_buf());
                }
                running.reporter.item(size(item));
            }
            Err(stop) => {
                result = Err(stop);
                break;
            }
        }
    }
    running.reporter.tell();
    // Only the folders the user sees: those that held the paths given.
    let shown: Vec<_> = paths
        .iter()
        .filter_map(|path| path.parent())
        .filter(|parent| changed.contains(*parent))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(PathBuf::from)
        .collect();
    (result.map_or_else(Stop::outcome, |()| Outcome::Done), shown)
}

/// `plan` as the journal keeps it. A delete that resumes keeps only what
/// it planned before the crash, entry for entry: what was added since,
/// even under a planned name, nobody chose to delete. One that starts
/// records its plan, on the disk before anything goes.
///
/// # Errors
/// [`Stop`] when cancelled while the plan is recorded.
fn planned(running: &mut Running, mut plan: Plan) -> Result<Plan, Stop> {
    if let Some(before) = &running.before.planned {
        plan.items
            .retain(|item| before.get(&item.from) == Some(&item.entry));
        plan.bytes = plan.items.iter().map(size).sum();
    } else {
        for item in &plan.items {
            running.shared.control.checkpoint()?;
            running.record(Record::Planned(&item.from, item.entry));
        }
        running.shared.control.checkpoint()?;
        running.record(Record::PlanDone);
        running.settle();
    }
    Ok(plan)
}

/// Removes `item`, asking what to do when it may not, and returns whether
/// it went.
///
/// # Errors
/// [`Stop`] when cancelled, or when a failure isn't the user's to answer.
fn remove(running: &mut Running, folders: &mut Folders<'_>, item: &Item) -> Result<bool, Stop> {
    loop {
        running.shared.control.checkpoint()?;
        let errno = match folders.remove(item) {
            Ok(()) => return Ok(true),
            // Gone already: what the user wanted.
            Err(Errno::NOENT) => return Ok(false),
            Err(Errno::NOTEMPTY | Errno::EXIST) => {
                log::info!("{} still holds something, so it stays", item.from.display());
                return Ok(false);
            }
            // A folder on the way became a link or a file since the plan:
            // what is there now isn't what the user chose.
            Err(Errno::LOOP | Errno::NOTDIR) => {
                log::info!(
                    "{} changed since it was planned, so it stays",
                    item.from.display()
                );
                return Ok(false);
            }
            Err(errno) => errno,
        };
        let reason = failure(&errno.into());
        let Some(kind) = conflict_for(reason, &item.from, &item.from) else {
            return Err(Stop::because(&item.from, reason));
        };
        let conflict = Conflict {
            kind,
            source: item.from.clone(),
            target: item.from.clone(),
            // Numbered when asked.
            number: 0,
        };
        if running.skips(conflict)? {
            return Ok(false);
        }
    }
}

/// The open folders of the trees being deleted. Each folder inside a tree
/// is opened from the one above it without following a link, so a folder
/// swapped for a link after the plan can't lead the delete out of the
/// tree the user chose.
struct Folders<'a> {
    /// The folders the user chose.
    trees: &'a HashSet<PathBuf>,
    open: HashMap<PathBuf, OwnedFd>,
}

impl Folders<'_> {
    /// Removes `item` from the folder it is in.
    fn remove(&mut self, item: &Item) -> rustix::io::Result<()> {
        let (Some(folder), Some(name)) = (item.from.parent(), item.from.file_name()) else {
            return Err(Errno::INVAL);
        };
        let flags = if item.kind == Kind::Folder {
            AtFlags::REMOVEDIR
        } else {
            AtFlags::empty()
        };
        self.reach(folder)?;
        let removed = self
            .open
            .get(folder)
            .map_or(Err(Errno::NOENT), |fd| unlinkat(fd, name, flags));
        // Only the folders on the way to this one stay open, so a delete
        // across many folders never holds more than one path's worth.
        self.open.retain(|open, _| folder.starts_with(open));
        removed
    }

    /// Opens `folder`, unless open already.
    fn reach(&mut self, folder: &Path) -> rustix::io::Result<()> {
        if self.open.contains_key(folder) {
            return Ok(());
        }
        // Only found, never read: removing needs a folder's write and
        // search permission, not leave to list it.
        let flags = OFlags::PATH.union(OFlags::DIRECTORY).union(OFlags::CLOEXEC);
        // By the folder's own ancestors, so a selection of many folders
        // costs one lookup each, not a scan of them all.
        let inside = folder.ancestors().any(|above| self.trees.contains(above));
        let fd = match (inside, folder.parent(), folder.file_name()) {
            (true, Some(above), Some(name)) => {
                self.reach(above)?;
                let above = self.open.get(above).ok_or(Errno::NOENT)?;
                openat(above, name, flags.union(OFlags::NOFOLLOW), Mode::empty())?
            }
            // Where the user's paths are: reached as the user named it.
            _ => open(folder, flags, Mode::empty())?,
        };
        self.open.insert(folder.to_path_buf(), fd);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use super::*;
    use crate::ops::control::Control;
    use crate::ops::test_folder::TestFolder;

    #[test]
    fn a_folder_swapped_for_a_link_is_not_followed() {
        let folder = TestFolder::new("delete-swap");
        let tree = folder.path().join("tree");
        fs::create_dir_all(tree.join("sub")).unwrap();
        fs::write(tree.join("sub/f"), "planned").unwrap();
        let outside = folder.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("f"), "kept").unwrap();
        let paths = [tree.clone()];
        let plan = plan_delete(&paths, &Control::default()).unwrap();
        // After the plan, `sub` becomes a link to a folder outside.
        fs::remove_dir_all(tree.join("sub")).unwrap();
        symlink(&outside, tree.join("sub")).unwrap();

        let mut folders = Folders {
            trees: &paths.iter().cloned().collect(),
            open: HashMap::new(),
        };
        let removed: Vec<_> = plan
            .items
            .iter()
            .rev()
            .map(|item| folders.remove(item))
            .collect();

        assert_eq!(fs::read_to_string(outside.join("f")).unwrap(), "kept");
        assert_eq!(
            removed,
            [Err(Errno::NOTDIR), Err(Errno::NOTDIR), Err(Errno::NOTEMPTY)],
            "f, then sub (now a link), then the tree that still holds it"
        );
    }

    #[test]
    fn a_delete_cancelled_while_its_plan_is_recorded_stops() {
        let folder = TestFolder::new("delete-plan-cancel");
        let paths = [folder.file("a", "a")];
        let plan = plan_delete(&paths, &Control::default()).unwrap();
        let mut running = Running::for_test();
        running.shared.control.cancel();

        assert!(planned(&mut running, plan).is_err());
    }

    #[test]
    fn a_resumed_delete_keeps_to_the_entries_it_planned() {
        let folder = TestFolder::new("delete-plan-entries");
        let paths = [folder.file("same", "a"), folder.file("other", "b")];
        let plan = plan_delete(&paths, &Control::default()).unwrap();
        let mut running = Running::for_test();
        let mut other = plan.items[1].entry;
        other.inode += 1;
        running.before.planned = Some(HashMap::from([
            (paths[0].clone(), plan.items[0].entry),
            (paths[1].clone(), other),
        ]));

        let kept = planned(&mut running, plan).unwrap();

        assert_eq!(kept.items.len(), 1);
        assert_eq!(kept.items[0].from, paths[0]);
        assert_eq!(kept.bytes, 1);
    }

    #[test]
    fn only_the_folders_on_the_way_stay_open() {
        let folder = TestFolder::new("delete-open");
        let paths: Vec<_> = ["a", "b", "c"]
            .iter()
            .map(|name| {
                fs::create_dir_all(folder.path().join(name).join("in")).unwrap();
                folder.file(&format!("{name}/in/f"), "f")
            })
            .collect();
        let plan = plan_delete(&paths, &Control::default()).unwrap();
        let mut folders = Folders {
            trees: &paths.iter().cloned().collect(),
            open: HashMap::new(),
        };
        for item in plan.items.iter().rev() {
            folders.remove(item).unwrap();
            let above = item.from.parent().unwrap();
            assert!(folders.open.keys().all(|open| above.starts_with(open)));
        }
        assert!(paths.iter().all(|path| !path.exists()));
    }
}
