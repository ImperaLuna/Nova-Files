use std::path::PathBuf;

use nova_files_api::{Failure, Operation, Outcome};
use nova_files_jobs::running::Running;
use nova_files_jobs::{Ended, JobKind};

use crate::undo::{self, Undo, undo_of};
use crate::{new_folder, rename};

pub struct Renames;

impl JobKind for Renames {
    type Op = Operation;
    type Undo = Undo;

    fn claims(&self, operation: &Operation) -> bool {
        matches!(
            operation,
            Operation::Rename { .. } | Operation::NewFolder { .. }
        )
    }

    fn run(&self, running: &mut Running, operation: &Operation, undo: Option<&Undo>) -> Ended {
        if let Some(undo) = undo {
            return undo::run(running, undo);
        }
        match operation {
            Operation::Rename { path, name } => rename(path, name),
            Operation::NewFolder { inside, name } => new_folder(inside, name),
            _ => (
                Outcome::Failed {
                    path: PathBuf::new(),
                    reason: Failure::Other,
                },
                Vec::new(),
            ),
        }
    }

    fn undo(&self, _: &mut Running, operation: &Operation, ended: &Ended) -> Option<Undo> {
        undo_of(operation, &ended.0)
    }

    fn first_path(&self, operation: &Operation) -> PathBuf {
        match operation {
            Operation::Rename { path, .. } => path.clone(),
            Operation::NewFolder { inside, name } => inside.join(name),
            _ => PathBuf::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renames_claim_renaming_and_new_folders_and_name_where_they_start() {
        let rename = Operation::Rename {
            path: "/a/b".into(),
            name: "c".into(),
        };
        let folder = Operation::NewFolder {
            inside: "/a".into(),
            name: "new".into(),
        };
        let delete = Operation::Delete { paths: Vec::new() };

        assert!(Renames.claims(&rename) && Renames.claims(&folder));
        assert!(!Renames.claims(&delete));
        assert_eq!(Renames.first_path(&rename), PathBuf::from("/a/b"));
        assert_eq!(Renames.first_path(&folder), PathBuf::from("/a/new"));
        assert_eq!(Renames.first_path(&delete), PathBuf::new());
    }
}
