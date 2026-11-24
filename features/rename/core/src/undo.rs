use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use nova_files_api::{Operation, Outcome};
use nova_files_jobs::Ended;
use nova_files_jobs::identity::{self, Identity, same};
use nova_files_jobs::running::Running;
use nova_files_jobs::stop::Stop;
use nova_files_jobs::undo::{gone, trash_again};
use nova_files_rename_api::{NewFileJob, NewFolderJob, RenameJob};

use crate::rename;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undo {
    Rename(PathBuf, Identity, OsString),
    RemoveFolder(PathBuf, Identity),
    TrashFile(PathBuf, Identity),
}

pub fn undo_of(operation: &Operation, outcome: &Outcome) -> Option<Undo> {
    if *outcome != Outcome::Done {
        return None;
    }
    if let Some(RenameJob { path, name }) = operation.get::<RenameJob>() {
        if path.file_name() == Some(name.as_os_str()) {
            return None;
        }
        let renamed = path.with_file_name(name);
        let entry = identity::of(&renamed)?;
        return Some(Undo::Rename(
            renamed,
            entry,
            path.file_name()?.to_os_string(),
        ));
    }
    if let Some(NewFileJob { inside, name }) = operation.get::<NewFileJob>() {
        let made = inside.join(name);
        let entry = identity::of(&made)?;
        return Some(Undo::TrashFile(made, entry));
    }
    let NewFolderJob { inside, name } = operation.get::<NewFolderJob>()?;
    let made = inside.join(name);
    let entry = identity::of(&made)?;
    Some(Undo::RemoveFolder(made, entry))
}

pub fn run(running: &mut Running, undo: &Undo) -> Ended {
    match undo {
        Undo::TrashFile(path, was) => trash_again(running, vec![(path.clone(), *was)]),
        Undo::Rename(path, was, name) => if_same(running, (path, *was), || rename(path, name)),
        Undo::RemoveFolder(path, was) => if_same(running, (path, *was), || remove_folder(path)),
    }
}

fn if_same(
    running: &Running,
    (path, was): (&Path, Identity),
    then: impl FnOnce() -> Ended,
) -> Ended {
    if !same(path, was) {
        return gone();
    }
    if let Err(stop) = running.shared.control.checkpoint().map_err(Stop::from) {
        return (stop.outcome(), Vec::new());
    }
    then()
}

fn remove_folder(path: &Path) -> Ended {
    match fs::remove_dir(path) {
        Ok(()) => (
            Outcome::Done,
            path.parent().map(Path::to_path_buf).into_iter().collect(),
        ),
        Err(error) => (Stop::at(path, &error).outcome(), Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use nova_files_api::Failure;
    use nova_files_test_folder::TestFolder;

    use super::*;

    #[test]
    fn a_rename_to_its_own_name_is_nothing_to_undo() {
        let folder = TestFolder::new("undo-same-name");
        let path = folder.file("a", "a");
        let rename_op = nova_files_rename_api::rename(path, "a".into());

        assert_eq!(undo_of(&rename_op, &Outcome::Done), None);
    }

    #[test]
    fn a_rename_and_a_new_folder_are_taken_back_only_once_done() {
        let folder = TestFolder::new("undo-one-step");
        let renamed = folder.file("new", "x");
        let rename_op = nova_files_rename_api::rename(folder.path().join("old"), "new".into());
        let made = folder.path().join("made");
        fs::create_dir(&made).unwrap();
        let folder_op =
            nova_files_rename_api::new_folder(folder.path().to_path_buf(), "made".into());
        let failed = Outcome::Failed {
            path: made.clone(),
            reason: Failure::NameTaken,
        };

        assert_eq!(
            undo_of(&rename_op, &Outcome::Done),
            Some(Undo::Rename(
                renamed.clone(),
                identity::of(&renamed).unwrap(),
                "old".into()
            ))
        );
        assert_eq!(
            undo_of(&folder_op, &Outcome::Done),
            Some(Undo::RemoveFolder(
                made.clone(),
                identity::of(&made).unwrap()
            ))
        );
        assert_eq!(undo_of(&rename_op, &failed), None);
        assert_eq!(undo_of(&folder_op, &failed), None);
    }
}
