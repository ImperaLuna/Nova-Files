use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use nova_files_api::{Failure, Outcome};
use nova_files_jobs::Ended;
use nova_files_jobs::failure::failure;
use nova_files_jobs::identity::{self, Identity, same};
use nova_files_jobs::place::rename_no_replace;
use nova_files_jobs::undo::gone;
use nova_files_rename_api::{RenameManyJob, is_plain_name};

use crate::failed;

pub type Renamed = Vec<(PathBuf, Identity, OsString)>;

struct Step {
    from: PathBuf,
    to: PathBuf,
}

pub fn rename_many(paths: &[PathBuf], names: &[OsString]) -> Ended {
    let mut waiting = match steps(paths, names) {
        Ok(steps) => steps,
        Err(path) => return failed(path, Failure::InvalidName),
    };
    let folders = folders_of(&waiting);
    let mut done: Vec<Step> = Vec::new();
    while !waiting.is_empty() {
        let held: HashSet<PathBuf> = waiting.iter().map(|step| step.from.clone()).collect();
        let (ready, rest): (Vec<Step>, Vec<Step>) = waiting
            .into_iter()
            .partition(|step| !held.contains(&step.to));
        if ready.is_empty() {
            let stuck = rest
                .first()
                .map_or_else(|| Path::new(""), |step| step.from.as_path());
            take_back(done);
            return failed(stuck, Failure::NameTaken);
        }
        waiting = rest;
        for step in ready {
            if let Err(error) = rename_no_replace(&step.from, &step.to) {
                take_back(done);
                return failed(&step.from, failure(&error));
            }
            done.push(step);
        }
    }
    (Outcome::Done, folders)
}

fn steps<'a>(paths: &'a [PathBuf], names: &[OsString]) -> Result<Vec<Step>, &'a Path> {
    let first = paths
        .first()
        .map_or_else(|| Path::new(""), PathBuf::as_path);
    if paths.len() != names.len() {
        return Err(first);
    }
    let mut steps = Vec::new();
    for (path, name) in paths.iter().zip(names) {
        let renames = path.is_absolute() && path.file_name().is_some() && is_plain_name(name);
        if !renames {
            return Err(path);
        }
        let to = path.with_file_name(name);
        if to != *path {
            steps.push(Step {
                from: path.clone(),
                to,
            });
        }
    }
    Ok(steps)
}

fn folders_of(steps: &[Step]) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = steps
        .iter()
        .filter_map(|step| step.from.parent().map(Path::to_path_buf))
        .collect();
    folders.sort();
    folders.dedup();
    folders
}

fn take_back(done: Vec<Step>) {
    for step in done.into_iter().rev() {
        rename_no_replace(&step.to, &step.from).ok();
    }
}

pub fn renamed(job: &RenameManyJob) -> Option<Renamed> {
    let renamed: Option<Renamed> = job
        .paths
        .iter()
        .zip(&job.names)
        .filter(|(path, name)| path.file_name() != Some(name.as_os_str()))
        .map(|(path, name)| {
            let now = path.with_file_name(name);
            let entry = identity::of(&now)?;
            Some((now, entry, path.file_name()?.to_os_string()))
        })
        .collect();
    renamed.filter(|renamed| !renamed.is_empty())
}

pub fn put_back(renamed: &Renamed) -> Ended {
    if !renamed.iter().all(|(path, was, _)| same(path, *was)) {
        return gone();
    }
    let (paths, names): (Vec<PathBuf>, Vec<OsString>) = renamed
        .iter()
        .map(|(path, _, name)| (path.clone(), name.clone()))
        .unzip();
    rename_many(&paths, &names)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use nova_files_test_folder::TestFolder;

    use super::*;

    fn job(folder: &TestFolder, renames: &[(&str, &str)]) -> RenameManyJob {
        RenameManyJob {
            paths: renames
                .iter()
                .map(|(from, _)| folder.path().join(from))
                .collect(),
            names: renames.iter().map(|(_, to)| OsString::from(to)).collect(),
        }
    }

    fn names(folder: &TestFolder) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(folder.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn every_file_takes_its_new_name_and_the_folder_is_named_once() {
        let folder = TestFolder::new("many-all");
        folder.file("a.txt", "a");
        folder.file("b.txt", "b");
        folder.file("c.txt", "c");
        let job = job(
            &folder,
            &[("a.txt", "x.txt"), ("b.txt", "y.txt"), ("c.txt", "c.txt")],
        );

        let ended = rename_many(&job.paths, &job.names);

        assert_eq!(ended, (Outcome::Done, vec![folder.path().to_path_buf()]));
        assert_eq!(names(&folder), ["c.txt", "x.txt", "y.txt"]);
    }

    #[test]
    fn a_name_another_file_is_leaving_is_taken_after_it_left() {
        let folder = TestFolder::new("many-chain");
        folder.file("x1", "one");
        folder.file("x11", "two");
        let job = job(&folder, &[("x1", "x11"), ("x11", "x111")]);

        assert_eq!(rename_many(&job.paths, &job.names).0, Outcome::Done);
        assert_eq!(names(&folder), ["x11", "x111"]);
        assert_eq!(
            fs::read_to_string(folder.path().join("x11")).unwrap(),
            "one"
        );
    }

    #[test]
    fn a_taken_name_puts_the_renamed_files_back() {
        let folder = TestFolder::new("many-taken");
        folder.file("a", "a");
        folder.file("b", "b");
        folder.file("kept", "kept");
        let job = job(&folder, &[("a", "z"), ("b", "kept")]);

        let ended = rename_many(&job.paths, &job.names);

        let failed = Outcome::Failed {
            path: folder.path().join("b"),
            reason: Failure::NameTaken,
        };
        assert_eq!(ended, (failed, Vec::new()));
        assert_eq!(names(&folder), ["a", "b", "kept"]);
        assert_eq!(
            fs::read_to_string(folder.path().join("kept")).unwrap(),
            "kept"
        );
    }

    #[test]
    fn two_files_swapping_names_rename_nothing() {
        let folder = TestFolder::new("many-swap");
        folder.file("a", "a");
        folder.file("b", "b");
        folder.file("c", "c");
        let job = job(&folder, &[("c", "d"), ("a", "b"), ("b", "a")]);

        let ended = rename_many(&job.paths, &job.names);

        assert!(matches!(
            ended.0,
            Outcome::Failed {
                reason: Failure::NameTaken,
                ..
            }
        ));
        assert_eq!(names(&folder), ["a", "b", "c"]);
        assert_eq!(fs::read_to_string(folder.path().join("a")).unwrap(), "a");
    }

    #[test]
    fn a_name_that_leaves_the_folder_renames_nothing() {
        let folder = TestFolder::new("many-invalid");
        folder.file("a", "a");
        folder.file("b", "b");
        let job = job(&folder, &[("a", "z"), ("b", "../b")]);

        let ended = rename_many(&job.paths, &job.names);

        let failed = Outcome::Failed {
            path: folder.path().join("b"),
            reason: Failure::InvalidName,
        };
        assert_eq!(ended, (failed, Vec::new()));
        assert_eq!(names(&folder), ["a", "b"]);
    }

    #[test]
    fn the_renamed_files_go_back_to_their_old_names() {
        let folder = TestFolder::new("many-undo");
        folder.file("x1", "one");
        folder.file("x11", "two");
        folder.file("same", "same");
        let job = job(&folder, &[("x1", "x11"), ("x11", "x111"), ("same", "same")]);
        rename_many(&job.paths, &job.names);

        let renamed = renamed(&job).unwrap();

        assert_eq!(renamed.len(), 2);
        assert_eq!(put_back(&renamed).0, Outcome::Done);
        assert_eq!(names(&folder), ["same", "x1", "x11"]);
        assert_eq!(fs::read_to_string(folder.path().join("x1")).unwrap(), "one");
    }

    #[test]
    fn files_changed_since_are_left_alone() {
        let folder = TestFolder::new("many-undo-gone");
        folder.file("a", "a");
        folder.file("b", "b");
        let job = job(&folder, &[("a", "x"), ("b", "y")]);
        rename_many(&job.paths, &job.names);
        let renamed = renamed(&job).unwrap();
        fs::remove_file(folder.path().join("y")).unwrap();

        assert_eq!(put_back(&renamed), gone());
        assert_eq!(names(&folder), ["x"]);
    }

    #[test]
    fn nothing_renamed_is_nothing_to_take_back() {
        let folder = TestFolder::new("many-undo-none");
        folder.file("a", "a");

        assert_eq!(renamed(&job(&folder, &[("a", "a")])), None);
    }
}
