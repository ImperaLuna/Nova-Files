use std::path::PathBuf;

use super::*;
use crate::ops::test_folder::TestFolder;

/// Places a file whose every try finds a name taken, at the path `taken`
/// names, as if another program kept taking it; returns what `place` did
/// and how often it tried. Past the most tries it may take, the stand-in
/// stops the job instead.
fn place_with_name_taken(taken: impl Fn(&Item) -> PathBuf) -> (Result<PathBuf, Stop>, u32) {
    let folder = TestFolder::new("transfer-look-again");
    let item = Item {
        from: folder.file("a", "a"),
        to: folder.path().join("b"),
        kind: Kind::File { size: 1 },
    };
    let mut running = Running::for_test();
    let mut tries = 0;
    let mut make = |_: &mut Running, making: &Making<'_>| {
        tries += 1;
        if tries > LOOK_AGAIN + 1 {
            return Err(Stop::Cancelled);
        }
        Err(Stop::because(&taken(making.item), Failure::NameTaken))
    };
    let planned = Making {
        item: &item,
        target: &item.to,
        index: 0,
        replacing: false,
        how: How::Copy,
    };
    let placed = place(&mut running, &planned, &mut make).map(|(_, at)| at);
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
