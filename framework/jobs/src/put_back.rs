use std::path::PathBuf;

use nova_files_api::{Failure, Outcome};

use crate::identity::{self, Identity};
use crate::journal::{Record, word};
use crate::runner::Ended;
use crate::running::Running;

pub type Replaced = Vec<(PathBuf, Identity)>;

pub fn kept(running: &Running) -> Replaced {
    running
        .replaced
        .iter()
        .filter_map(|(_, item)| Some((item.clone(), identity::of(item)?)))
        .collect()
}

pub fn after(running: &mut Running, (outcome, mut changed): Ended, replaced: &Replaced) -> Ended {
    let free = outcome == Outcome::Done || nothing_left(&outcome);
    let items = identity::still(replaced.clone());
    if !free || items.is_empty() {
        return (outcome, changed);
    }
    running.record(Record::Operation(word::PUT_BACK));
    let Some((put, more)) = running.put_back(&items) else {
        return (outcome, changed);
    };
    changed.extend(more);
    changed.sort();
    changed.dedup();
    (put, changed)
}

fn nothing_left(outcome: &Outcome) -> bool {
    matches!(outcome, Outcome::Failed { path, reason: Failure::NotFound } if path.as_os_str().is_empty())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::set_aside::{Aside, SetAsideForJob};
    use crate::test_folder::TestFolder;

    struct Shelf {
        asked: Arc<Mutex<Vec<PathBuf>>>,
    }

    impl SetAsideForJob for Shelf {
        fn set_aside(&mut self, _: &mut Running, _: (usize, &Path), _: &Path) -> Aside {
            Aside::Nowhere
        }

        fn trash(&mut self, _: &mut Running, _: &[PathBuf]) -> Ended {
            (Outcome::Done, Vec::new())
        }

        fn put_back(&mut self, _: &mut Running, items: &[PathBuf]) -> Option<Ended> {
            self.asked.lock().unwrap().extend_from_slice(items);
            Some((Outcome::Done, vec![PathBuf::from("/put")]))
        }
    }

    fn running() -> (Running, Arc<Mutex<Vec<PathBuf>>>) {
        let asked = Arc::default();
        let mut running = Running::for_test();
        running.set_aside_with(Box::new(Shelf {
            asked: Arc::clone(&asked),
        }));
        (running, asked)
    }

    fn undone() -> Ended {
        (Outcome::Done, vec![PathBuf::from("/undone")])
    }

    #[test]
    fn files_replaced_and_still_there_are_put_back_after_the_undo() {
        let folder = TestFolder::new("put-back-after");
        let kept_aside = folder.file("old-a", "old");
        let (mut running, asked) = running();
        running
            .replaced
            .push((folder.path().join("a"), kept_aside.clone()));
        running
            .replaced
            .push((folder.path().join("b"), folder.path().join("gone")));
        let replaced = kept(&running);

        let (outcome, changed) = after(&mut running, undone(), &replaced);

        assert_eq!(*asked.lock().unwrap(), vec![kept_aside]);
        assert_eq!(outcome, Outcome::Done);
        assert_eq!(
            changed,
            vec![PathBuf::from("/put"), PathBuf::from("/undone")]
        );
    }

    #[test]
    fn an_undo_with_nothing_left_still_puts_back() {
        let folder = TestFolder::new("put-back-nothing-left");
        let kept_aside = folder.file("old-a", "old");
        let (mut running, asked) = running();
        running
            .replaced
            .push((folder.path().join("a"), kept_aside.clone()));
        let replaced = kept(&running);
        let nothing = Outcome::Failed {
            path: PathBuf::new(),
            reason: Failure::NotFound,
        };

        after(&mut running, (nothing, Vec::new()), &replaced);

        assert_eq!(*asked.lock().unwrap(), vec![kept_aside]);
    }

    #[test]
    fn a_failed_undo_or_a_changed_file_puts_nothing_back() {
        let folder = TestFolder::new("put-back-failed");
        let kept_aside = folder.file("old-a", "old");
        let (mut running, asked) = running();
        running
            .replaced
            .push((folder.path().join("a"), kept_aside.clone()));
        let replaced = kept(&running);
        let refused = Outcome::Failed {
            path: PathBuf::from("/a"),
            reason: Failure::NotFound,
        };

        let ended = after(&mut running, (refused.clone(), Vec::new()), &replaced);
        assert_eq!(ended, (refused, Vec::new()));
        std::fs::remove_file(&kept_aside).unwrap();
        folder.file("old-a", "someone else's");
        let ended = after(&mut running, undone(), &replaced);

        assert_eq!(ended, undone());
        assert_eq!(*asked.lock().unwrap(), Vec::<PathBuf>::new());
    }
}
