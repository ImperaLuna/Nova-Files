use std::path::PathBuf;
use std::sync::Arc;

use nova_files_api::{JobId, Operation};

use super::*;
use crate::ops::audience::Audience;
use crate::ops::job::Job;
use crate::ops::progress::Reporter;
use crate::ops::test_folder::TestFolder;

/// Places a file whose every try finds a name taken, at the path `taken`
/// names, as if another program kept taking it; returns what `place` did
/// and how often it tried. Past the most tries it may take, the stand-in
/// stops the job instead.
fn place_with_name_taken(taken: impl Fn(&Item) -> PathBuf) -> (Result<PathBuf, Stop>, u32) {
    let folder = TestFolder::new("copy-look-again");
    let item = Item {
        from: folder.file("a", "a"),
        to: folder.path().join("b"),
        kind: Kind::File { size: 1 },
    };
    let shared = Arc::new(Job::new(Operation::Copy {
        sources: Vec::new(),
        into: folder.path().to_path_buf(),
    }));
    let reporter = Reporter::new(
        Audience::default(),
        Arc::default(),
        JobId(1),
        Arc::clone(&shared),
    );
    let mut running = Running::new(JobId(1), shared, reporter, None);
    let mut tries = 0;
    let mut make = |_: &mut Running, item: &Item, _: &Path, _: usize, _: bool| {
        tries += 1;
        if tries > LOOK_AGAIN + 1 {
            return Err(Stop::Cancelled);
        }
        Err(Stop::because(&taken(item), Failure::NameTaken))
    };
    let placed = place(&mut running, &item, &item.to, 0, &mut make).map(|(_, at)| at);
    assert!(!item.to.exists());
    (placed, tries)
}

#[test]
fn a_name_taken_after_it_was_looked_at_is_looked_at_again_a_few_times() {
    let (placed, tries) = place_with_name_taken(|item| item.to.clone());

    assert!(matches!(
        placed,
        Err(Stop::Failed { ref path, reason: Failure::NameTaken }) if path.ends_with("b")
    ));
    assert_eq!(tries, LOOK_AGAIN + 1, "the first try and each look again");
}

#[test]
fn only_a_taken_target_is_looked_at_again() {
    let (placed, tries) = place_with_name_taken(|item| item.from.with_file_name("c"));

    assert!(matches!(
        placed,
        Err(Stop::Failed { ref path, reason: Failure::NameTaken }) if path.ends_with("c")
    ));
    assert_eq!(tries, 1);
}
