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

#[test]
fn only_what_a_folder_rename_takes_along_is_carried() {
    let folder = TestFolder::new("transfer-carried");
    let a = folder.path().join("a");
    let elsewhere = folder.path().join("e");
    fs::create_dir_all(a.join("b")).unwrap();
    fs::create_dir(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, a.join("link")).unwrap();
    let alias = folder.path().join("alias");
    std::os::unix::fs::symlink(&a, &alias).unwrap();

    let kept = |sources: &[PathBuf]| outermost(sources, &Control::default()).unwrap();
    let inside = [a.clone(), a.join("b"), a.join("b/f"), a.join("link/f")];
    assert_eq!(
        kept(&inside),
        [a.clone(), a.join("link/f")],
        "through a link inside"
    );
    assert_eq!(kept(&[a.clone(), a]).len(), 2, "not itself");
    let through_alias = [alias.clone(), alias.join("b")];
    assert_eq!(
        kept(&through_alias),
        through_alias,
        "inside a link to a folder"
    );
    let control = Control::default();
    control.cancel();
    assert!(outermost(&inside, &control).is_err());
}

#[test]
fn a_folder_a_move_emptied_goes_unless_something_keeps_it() {
    let folder = TestFolder::new("transfer-emptied");
    let mut running = Running::for_test();
    let mut touched = BTreeSet::new();
    let empty = folder.path().join("empty");
    fs::create_dir(&empty).unwrap();
    remove_if_empty(&mut running, &empty, &mut touched).unwrap();
    assert!(!empty.exists());
    assert_eq!(touched, BTreeSet::from([folder.path().to_path_buf()]));

    let full = folder.path().join("full");
    fs::create_dir(&full).unwrap();
    folder.file("full/f", "f");
    let gone = folder.path().join("gone");
    let mut nothing = BTreeSet::new();
    remove_if_empty(&mut running, &full, &mut nothing).unwrap();
    remove_if_empty(&mut running, &gone, &mut nothing).unwrap();
    assert!(full.exists());
    assert!(nothing.is_empty());

    let file = folder.file("file", "f");
    assert!(matches!(
        remove_if_empty(&mut running, &file, &mut nothing),
        Err(Stop::Failed {
            reason: Failure::Other,
            ..
        })
    ));
    fs::create_dir(&empty).unwrap();
    running.shared.control.cancel();
    assert!(remove_if_empty(&mut running, &empty, &mut nothing).is_err());
    assert!(empty.exists(), "a cancel leaves it");
}
