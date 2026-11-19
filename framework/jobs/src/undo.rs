use std::path::PathBuf;

use nova_files_api::{Failure, Outcome};

use crate::identity::{self, Identity, still};
use crate::runner::Ended;
use crate::running::Running;

pub fn landed(running: &mut Running) -> Vec<(PathBuf, PathBuf, Identity)> {
    std::mem::take(&mut running.steps)
        .into_iter()
        .filter_map(|(from, to)| {
            let made = identity::of(&to)?;
            Some((to, from, made))
        })
        .collect()
}

pub fn trash_again(running: &mut Running, made: Vec<(PathBuf, Identity)>) -> Ended {
    match still(made) {
        left if left.is_empty() => gone(),
        left => running.trash(&left),
    }
}

pub const fn gone() -> Ended {
    let outcome = Outcome::Failed {
        path: PathBuf::new(),
        reason: Failure::NotFound,
    };
    (outcome, Vec::new())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use nova_files_api::{JobId, Operation};

    use super::*;
    use crate::audience::Audience;
    use crate::job::Job;
    use crate::progress::Reporter;
    use crate::test_folder::TestFolder;

    fn running() -> Running {
        let shared = Arc::new(Job::new(Operation::Copy {
            sources: Vec::new(),
            into: "/".into(),
        }));
        let reporter = Reporter::new(
            Audience::default(),
            Arc::default(),
            JobId(1),
            Arc::clone(&shared),
        );
        Running::new(JobId(1), shared, reporter, None)
    }

    #[test]
    fn only_the_steps_whose_target_is_still_there_have_landed() {
        let folder = TestFolder::new("undo-landed");
        let kept = folder.file("kept", "one");
        let lost = folder.file("lost", "two");
        let mut running = running();
        running.steps.insert("/from/kept".into(), kept.clone());
        running.steps.insert("/from/lost".into(), lost.clone());
        fs::remove_file(&lost).unwrap();

        let landed = landed(&mut running);

        assert_eq!(landed.len(), 1);
        assert_eq!((&landed[0].0, &landed[0].1), (&kept, &"/from/kept".into()));
        assert_eq!(identity::of(&kept), Some(landed[0].2));
        assert!(running.steps.is_empty());
    }

    #[test]
    fn nothing_left_to_trash_again_is_gone() {
        let folder = TestFolder::new("undo-gone");
        let file = folder.file("a", "one");
        let was = identity::of(&file).unwrap();
        fs::remove_file(&file).unwrap();
        folder.file("a", "other");

        assert_eq!(trash_again(&mut running(), vec![(file, was)]), gone());
        assert_eq!(
            gone().0,
            Outcome::Failed {
                path: PathBuf::new(),
                reason: Failure::NotFound
            }
        );
    }
}
