#![forbid(unsafe_code)]

mod kind;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use nova_files_api::Outcome;
use rustix::fs::{AtFlags, unlinkat};
use rustix::io::Errno;

use nova_files_jobs::Ended;
use nova_files_jobs::failure::failure;
use nova_files_jobs::folders::{Folders, Unreached};
use nova_files_jobs::journal::{Record, word};
use nova_files_jobs::plan::{Item, Kind, Plan, plan_delete};
use nova_files_jobs::running::Running;
use nova_files_jobs::stop::Stop;

pub use kind::Deletes;

pub fn delete(running: &mut Running, paths: &[PathBuf]) -> Ended {
    running.record_job(word::DELETE, None, paths);
    let (result, shown) = delete_paths(running, paths);
    (result.map_or_else(Stop::outcome, |()| Outcome::Done), shown)
}

pub fn delete_paths(running: &mut Running, paths: &[PathBuf]) -> (Result<(), Stop>, Vec<PathBuf>) {
    let mut ordered = paths.to_vec();
    ordered.sort_by_key(|path| path.components().count());
    let before = running.before.planned.clone();
    let wanted = |path: &Path| {
        before
            .as_ref()
            .is_none_or(|before| before.contains_key(path))
    };
    let plan = match plan_delete(&ordered, &running.shared.control, &wanted) {
        Ok(plan) => plan,
        Err(stop) => return (Err(stop), Vec::new()),
    };
    let mut folders = Folders::of(&plan);
    let plan = match planned(running, plan) {
        Ok(plan) => plan,
        Err(stop) => return (Err(stop), Vec::new()),
    };
    let items = u64::try_from(plan.items.len()).unwrap_or(u64::MAX);
    running.reporter.planned(plan.bytes, items);
    let mut changed = BTreeSet::new();
    let mut result = Ok(());
    for item in plan.items.iter().rev() {
        match remove(running, &mut folders, item) {
            Ok(removed) => {
                if removed && let Some(parent) = item.from.parent() {
                    changed.insert(parent.to_path_buf());
                }
                running.reporter.item(item.size());
            }
            Err(stop) => {
                result = Err(stop);
                break;
            }
        }
    }
    running.reporter.tell();
    let shown: Vec<_> = paths
        .iter()
        .filter_map(|path| path.parent())
        .filter(|parent| changed.contains(*parent))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(PathBuf::from)
        .collect();
    (result, shown)
}

fn planned(running: &mut Running, mut plan: Plan) -> Result<Plan, Stop> {
    if let Some(before) = &running.before.planned {
        plan.items.retain(|item| {
            before
                .get(&item.from)
                .is_some_and(|planned| planned.is(&item.entry))
        });
        plan.bytes = plan.items.iter().map(Item::size).sum();
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

fn remove(running: &mut Running, folders: &mut Folders, item: &Item) -> Result<bool, Stop> {
    let went = running.retry_or_skip(&item.from, || match remove_from(folders, item) {
        Ok(()) => Ok(true),
        Err(Unreached::Failed(Errno::NOENT)) => Ok(false),
        Err(Unreached::Failed(Errno::NOTEMPTY | Errno::EXIST)) => {
            log::info!("{} still holds something, so it stays", item.from.display());
            Ok(false)
        }
        Err(Unreached::Changed) => {
            log::info!(
                "{} changed since it was planned, so it stays",
                item.from.display()
            );
            Ok(false)
        }
        Err(Unreached::Failed(errno)) => Err(failure(&errno.into())),
    })?;
    Ok(went.unwrap_or(false))
}

fn remove_from(folders: &mut Folders, item: &Item) -> Result<(), Unreached> {
    let flags = if item.kind == Kind::Folder {
        AtFlags::REMOVEDIR
    } else {
        AtFlags::empty()
    };
    let source = folders.source(&item.from)?;
    let at = source.at();
    Ok(unlinkat(at.folder, at.name, flags)?)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::os::unix::fs::symlink;

    use super::*;
    use nova_files_jobs::control::Control;
    use nova_files_jobs::test_folder::TestFolder;

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
        let plan = plan_delete(&paths, &Control::default(), &|_| true).unwrap();
        fs::remove_dir_all(tree.join("sub")).unwrap();
        symlink(&outside, tree.join("sub")).unwrap();

        let mut folders = Folders::of(&plan);
        let removed: Vec<_> = plan
            .items
            .iter()
            .rev()
            .map(|item| remove_from(&mut folders, item))
            .collect();

        assert_eq!(fs::read_to_string(outside.join("f")).unwrap(), "kept");
        assert_eq!(
            removed,
            [
                Err(Unreached::Changed),
                Err(Unreached::Changed),
                Err(Unreached::Failed(Errno::NOTEMPTY))
            ],
            "f, then sub (now a link), then the tree that still holds it"
        );
    }

    #[test]
    fn a_delete_cancelled_while_its_plan_is_recorded_stops() {
        let folder = TestFolder::new("delete-plan-cancel");
        let paths = [folder.file("a", "a")];
        let plan = plan_delete(&paths, &Control::default(), &|_| true).unwrap();
        let mut running = Running::for_test();
        running.shared.control.cancel();

        assert!(planned(&mut running, plan).is_err());
    }

    #[test]
    fn a_resumed_delete_keeps_to_the_entries_it_planned() {
        let folder = TestFolder::new("delete-plan-entries");
        let paths = [folder.file("same", "a"), folder.file("other", "b")];
        let plan = plan_delete(&paths, &Control::default(), &|_| true).unwrap();
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
    fn a_resumed_delete_leaves_a_new_entry_that_took_a_planned_inode() {
        let folder = TestFolder::new("delete-plan-reused");
        let paths = [folder.file("a", "a")];
        let mut plan = plan_delete(&paths, &Control::default(), &|_| true).unwrap();
        let mut running = Running::for_test();
        let mut before = plan.items[0].entry;
        before.born = Some(1);
        plan.items[0].entry.born = Some(2);
        running.before.planned = Some(HashMap::from([(paths[0].clone(), before)]));

        let kept = planned(&mut running, plan).unwrap();

        assert_eq!(kept.items, []);
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
        let plan = plan_delete(&paths, &Control::default(), &|_| true).unwrap();
        let mut folders = Folders::of(&plan);
        for item in plan.items.iter().rev() {
            remove_from(&mut folders, item).unwrap();
            let above = item.from.parent().unwrap();
            assert!(folders.opened().all(|open| above.starts_with(open)));
        }
        assert!(paths.iter().all(|path| !path.exists()));
    }
}
