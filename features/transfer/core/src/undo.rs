use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use nova_files_api::{Operation, Outcome};
use nova_files_jobs::Ended;
use nova_files_jobs::identity::{Identity, same};
use nova_files_jobs::journal::{Record, word};
use nova_files_jobs::parents::make_folders;
use nova_files_jobs::running::Running;
use nova_files_jobs::stop::Stop;
use nova_files_jobs::undo::{gone, trash_again};

use crate::{How, transfer_to};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undo {
    Trash(Vec<(PathBuf, Identity)>),
    MoveBack(Vec<(PathBuf, PathBuf, Identity)>),
}

pub fn undo_of(operation: &Operation, landed: Vec<(PathBuf, PathBuf, Identity)>) -> Option<Undo> {
    if landed.is_empty() {
        return None;
    }
    match operation {
        Operation::Copy { .. } => Some(Undo::Trash(
            landed.into_iter().map(|(to, _, made)| (to, made)).collect(),
        )),
        Operation::Move { .. } => Some(Undo::MoveBack(landed)),
        _ => None,
    }
}

pub fn run(running: &mut Running, undo: &Undo) -> Ended {
    match undo {
        Undo::Trash(made) => trash_again(running, made.clone()),
        Undo::MoveBack(pairs) => {
            let pairs: Vec<_> = pairs
                .iter()
                .filter(|(to, _, was)| same(to, *was))
                .map(|(to, from, _)| (to.clone(), from.clone()))
                .collect();
            if pairs.is_empty() {
                return gone();
            }
            move_back(running, &pairs)
        }
    }
}

fn move_back(running: &mut Running, pairs: &[(PathBuf, PathBuf)]) -> Ended {
    running.record(Record::Operation(word::UNDO));
    let mut made = BTreeSet::new();
    let mut targets: Vec<_> = pairs.iter().map(|(_, from)| from.as_path()).collect();
    targets.sort_by_key(|target| target.components().count());
    if let Err(stop) = make_folders(running, &targets, &mut made) {
        return (stop.outcome(), made.into_iter().collect());
    }
    let (result, touched) = transfer_to(running, pairs, How::Move);
    let shown = pairs
        .iter()
        .flat_map(|(to, from)| [to.parent(), from.parent()])
        .flatten()
        .filter(|folder| touched.contains(*folder))
        .map(Path::to_path_buf)
        .chain(made)
        .collect::<BTreeSet<_>>();
    (
        result.map_or_else(Stop::outcome, |()| Outcome::Done),
        shown.into_iter().collect(),
    )
}

#[cfg(test)]
mod tests {
    use nova_files_jobs::JobKind as _;
    use nova_files_jobs::identity;
    use nova_files_test_folder::TestFolder;

    use super::*;
    use crate::Transfers;

    fn undo_after(
        operation: &Operation,
        steps: Vec<(PathBuf, PathBuf)>,
        outcome: Outcome,
    ) -> Option<Undo> {
        let mut running = Running::for_test();
        running.steps.extend(steps);
        Transfers.undo(&mut running, operation, &(outcome, Vec::new()))
    }

    #[test]
    fn a_copy_sends_its_copies_still_there_to_the_trash() {
        let folder = TestFolder::new("undo-copy");
        let copy = folder.file("a (2)", "a");
        let source = folder.path().join("a");
        let copy_op = Operation::Copy {
            sources: vec![source.clone()],
            into: folder.path().to_path_buf(),
        };
        let steps = vec![
            (source, copy.clone()),
            (folder.path().join("b"), folder.path().join("gone")),
        ];

        let undo = undo_after(&copy_op, steps, Outcome::Done);

        assert_eq!(
            undo,
            Some(Undo::Trash(vec![(
                copy.clone(),
                identity::of(&copy).unwrap()
            )]))
        );
    }

    #[test]
    fn a_move_goes_back_even_when_it_stopped_part_way() {
        let folder = TestFolder::new("undo-move");
        let moved = folder.file("b", "b");
        let from = folder.path().join("from/b");
        let move_op = Operation::Move {
            sources: vec![from.clone(), folder.path().join("from/c")],
            into: folder.path().to_path_buf(),
        };

        let undo = undo_after(
            &move_op,
            vec![(from.clone(), moved.clone())],
            Outcome::Cancelled,
        );

        assert_eq!(
            undo,
            Some(Undo::MoveBack(vec![(
                moved.clone(),
                from,
                identity::of(&moved).unwrap()
            )]))
        );
    }

    #[test]
    fn nothing_landed_is_nothing_to_undo() {
        let folder = TestFolder::new("undo-copy-nothing");
        let copy_op = Operation::Copy {
            sources: vec![folder.path().join("a")],
            into: folder.path().to_path_buf(),
        };

        assert_eq!(undo_after(&copy_op, Vec::new(), Outcome::Done), None);
    }
}
