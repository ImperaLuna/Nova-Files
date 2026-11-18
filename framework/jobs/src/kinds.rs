use std::any::Any;
use std::path::PathBuf;
use std::sync::Arc;

use nova_files_api::{Failure, JobId, Operation, Outcome};

use crate::control::{Cancelled, Control};
use crate::runner::{Ended, JobKind};
use crate::running::Running;

pub type AnyUndo = Arc<dyn Any + Send + Sync>;

#[derive(Clone)]
pub struct KindUndo {
    kind: usize,
    undo: AnyUndo,
}

trait Erased: Send + Sync {
    fn claims(&self, operation: &Operation) -> bool;
    fn run(&self, running: &mut Running, operation: &Operation, undo: Option<&AnyUndo>) -> Ended;
    fn rest(&self, operation: &Operation, control: &Control) -> Result<Operation, Cancelled>;
    fn undo(&self, running: &mut Running, operation: &Operation, ended: &Ended) -> Option<AnyUndo>;
    fn undoes(&self, operation: &Operation) -> Option<JobId>;
    fn first_path(&self, operation: &Operation) -> PathBuf;
}

impl<K> Erased for K
where
    K: JobKind<Op = Operation>,
    K::Undo: Sync,
{
    fn claims(&self, operation: &Operation) -> bool {
        JobKind::claims(self, operation)
    }

    fn run(&self, running: &mut Running, operation: &Operation, undo: Option<&AnyUndo>) -> Ended {
        let undo = undo.and_then(|undo| undo.downcast_ref::<K::Undo>());
        JobKind::run(self, running, operation, undo)
    }

    fn rest(&self, operation: &Operation, control: &Control) -> Result<Operation, Cancelled> {
        JobKind::rest(self, operation, control)
    }

    fn undo(&self, running: &mut Running, operation: &Operation, ended: &Ended) -> Option<AnyUndo> {
        JobKind::undo(self, running, operation, ended).map(|undo| Arc::new(undo) as AnyUndo)
    }

    fn undoes(&self, operation: &Operation) -> Option<JobId> {
        JobKind::undoes(self, operation)
    }

    fn first_path(&self, operation: &Operation) -> PathBuf {
        JobKind::first_path(self, operation)
    }
}

#[derive(Default)]
pub struct Kinds {
    kinds: Vec<Box<dyn Erased>>,
}

impl Kinds {
    #[must_use]
    pub fn with<K>(mut self, kind: K) -> Self
    where
        K: JobKind<Op = Operation>,
        K::Undo: Sync,
    {
        self.kinds.push(Box::new(kind));
        self
    }

    fn claiming(&self, operation: &Operation) -> Option<(usize, &dyn Erased)> {
        self.kinds
            .iter()
            .enumerate()
            .find(|(_, kind)| kind.claims(operation))
            .map(|(at, kind)| (at, kind.as_ref()))
    }
}

impl JobKind for Kinds {
    type Op = Operation;
    type Undo = KindUndo;

    fn claims(&self, operation: &Operation) -> bool {
        self.claiming(operation).is_some()
    }

    fn run(&self, running: &mut Running, operation: &Operation, undo: Option<&KindUndo>) -> Ended {
        if let Some(undo) = undo
            && let Some(kind) = self.kinds.get(undo.kind)
        {
            return kind.run(running, operation, Some(&undo.undo));
        }
        match self.claiming(operation) {
            Some((_, kind)) => kind.run(running, operation, None),
            None => (
                Outcome::Failed {
                    path: JobKind::first_path(self, operation),
                    reason: Failure::Other,
                },
                Vec::new(),
            ),
        }
    }

    fn rest(&self, operation: &Operation, control: &Control) -> Result<Operation, Cancelled> {
        self.claiming(operation).map_or_else(
            || Ok(operation.clone()),
            |(_, kind)| kind.rest(operation, control),
        )
    }

    fn undo(
        &self,
        running: &mut Running,
        operation: &Operation,
        ended: &Ended,
    ) -> Option<KindUndo> {
        let (at, kind) = self.claiming(operation)?;
        let undo = kind.undo(running, operation, ended)?;
        Some(KindUndo { kind: at, undo })
    }

    fn undoes(&self, operation: &Operation) -> Option<JobId> {
        self.kinds.iter().find_map(|kind| kind.undoes(operation))
    }

    fn first_path(&self, operation: &Operation) -> PathBuf {
        self.claiming(operation)
            .map_or_else(PathBuf::new, |(_, kind)| kind.first_path(operation))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct Renames {
        undone_with: Arc<Mutex<Option<String>>>,
    }

    struct Deletes;

    impl JobKind for Renames {
        type Op = Operation;
        type Undo = String;

        fn claims(&self, operation: &Operation) -> bool {
            matches!(
                operation,
                Operation::Rename { .. } | Operation::Undo { job: JobId(1) }
            )
        }

        fn run(&self, _: &mut Running, _: &Operation, undo: Option<&String>) -> Ended {
            *self.undone_with.lock().unwrap() = undo.cloned();
            (Outcome::Done, Vec::new())
        }

        fn rest(&self, operation: &Operation, _: &Control) -> Result<Operation, Cancelled> {
            Ok(operation.clone())
        }

        fn undo(&self, _: &mut Running, _: &Operation, _: &Ended) -> Option<String> {
            Some("old name".to_owned())
        }

        fn undoes(&self, operation: &Operation) -> Option<JobId> {
            match operation {
                Operation::Undo { job } => Some(*job),
                _ => None,
            }
        }

        fn first_path(&self, _: &Operation) -> PathBuf {
            PathBuf::from("/renamed")
        }
    }

    impl JobKind for Deletes {
        type Op = Operation;
        type Undo = ();

        fn claims(&self, operation: &Operation) -> bool {
            matches!(operation, Operation::Delete { .. })
        }

        fn run(&self, _: &mut Running, _: &Operation, _: Option<&()>) -> Ended {
            (Outcome::Cancelled, Vec::new())
        }

        fn rest(&self, operation: &Operation, _: &Control) -> Result<Operation, Cancelled> {
            Ok(operation.clone())
        }

        fn undo(&self, _: &mut Running, _: &Operation, _: &Ended) -> Option<()> {
            None
        }

        fn undoes(&self, _: &Operation) -> Option<JobId> {
            None
        }

        fn first_path(&self, _: &Operation) -> PathBuf {
            PathBuf::from("/deleted")
        }
    }

    fn rename() -> Operation {
        Operation::Rename {
            path: PathBuf::from("/a"),
            name: "b".into(),
        }
    }

    #[test]
    fn each_operation_runs_on_the_kind_that_claims_it() {
        let kinds = Kinds::default().with(Deletes).with(Renames::default());
        let mut running = Running::for_test();
        let delete = Operation::Delete {
            paths: vec![PathBuf::from("/a")],
        };

        assert_eq!(
            JobKind::run(&kinds, &mut running, &delete, None).0,
            Outcome::Cancelled
        );
        assert_eq!(
            JobKind::run(&kinds, &mut running, &rename(), None).0,
            Outcome::Done
        );
        assert_eq!(
            JobKind::first_path(&kinds, &delete),
            PathBuf::from("/deleted")
        );
        assert_eq!(
            JobKind::first_path(&kinds, &rename()),
            PathBuf::from("/renamed")
        );
    }

    #[test]
    fn an_undo_goes_back_to_the_kind_that_made_it() {
        let undone_with = Arc::new(Mutex::new(None));
        let renames = Renames {
            undone_with: Arc::clone(&undone_with),
        };
        let kinds = Kinds::default().with(Deletes).with(renames);
        let mut running = Running::for_test();
        let ended = (Outcome::Done, Vec::new());

        let undo = JobKind::undo(&kinds, &mut running, &rename(), &ended).unwrap();
        let undoing = Operation::Undo { job: JobId(1) };
        assert_eq!(JobKind::undoes(&kinds, &undoing), Some(JobId(1)));
        JobKind::run(&kinds, &mut running, &undoing, Some(&undo));

        assert_eq!(*undone_with.lock().unwrap(), Some("old name".to_owned()));
    }

    #[test]
    fn an_operation_no_kind_claims_fails_without_running() {
        let kinds = Kinds::default().with(Deletes);
        let mut running = Running::for_test();

        let (outcome, changed) = JobKind::run(&kinds, &mut running, &rename(), None);

        assert_eq!(
            outcome,
            Outcome::Failed {
                path: PathBuf::new(),
                reason: Failure::Other,
            }
        );
        assert_eq!(changed, Vec::<PathBuf>::new());
        assert!(
            JobKind::undo(
                &kinds,
                &mut running,
                &rename(),
                &(Outcome::Done, Vec::new())
            )
            .is_none()
        );
    }
}
