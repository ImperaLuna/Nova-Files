use std::fs;
use std::path::PathBuf;

use super::*;
use nova_files_jobs::plan::Entry;
use nova_files_jobs::test_folder::TestFolder;

fn place_with_name_taken(taken: impl Fn(&Item) -> PathBuf) -> (Result<PathBuf, Stop>, u32) {
    let folder = TestFolder::new("transfer-look-again");
    let item = Item {
        from: folder.file("a", "a"),
        to: folder.path().join("b"),
        kind: Kind::File { size: 1 },
        entry: Entry::default(),
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
    let items = [item.clone()];
    let mut ahead = Ahead::new(&items);
    let placed = place(&mut running, &planned, &mut make, &mut ahead).map(|(_, at)| at);
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
    remove_if_empty(&mut running, &mut Folders::default(), &empty, &mut touched).unwrap();
    assert!(!empty.exists());
    assert_eq!(touched, BTreeSet::from([folder.path().to_path_buf()]));

    let full = folder.path().join("full");
    fs::create_dir(&full).unwrap();
    folder.file("full/f", "f");
    let gone = folder.path().join("gone");
    let mut nothing = BTreeSet::new();
    remove_if_empty(&mut running, &mut Folders::default(), &full, &mut nothing).unwrap();
    remove_if_empty(&mut running, &mut Folders::default(), &gone, &mut nothing).unwrap();
    assert!(full.exists());
    assert!(nothing.is_empty());

    let file = folder.file("file", "f");
    remove_if_empty(&mut running, &mut Folders::default(), &file, &mut nothing).unwrap();
    assert!(file.exists());
    assert!(matches!(
        remove_if_empty(
            &mut running,
            &mut Folders::default(),
            &folder.path().join("in\0valid"),
            &mut nothing
        ),
        Err(Stop::Failed {
            reason: Failure::Other,
            ..
        })
    ));
    fs::create_dir(&empty).unwrap();
    running.shared.control.cancel();
    assert!(remove_if_empty(&mut running, &mut Folders::default(), &empty, &mut nothing).is_err());
    assert!(empty.exists(), "a cancel leaves it");
}

#[test]
fn folders_already_made_get_their_mode_and_times_after_a_cancel() {
    use std::os::unix::fs::PermissionsExt as _;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant, SystemTime};

    const ASK_WAIT: Duration = Duration::from_secs(5);

    let folder = TestFolder::new("transfer-finish-cancel");
    let made = folder.path().join("made");
    fs::create_dir(&made).unwrap();
    fs::set_permissions(&made, fs::Permissions::from_mode(0o750)).unwrap();
    let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    File::open(&made).unwrap().set_modified(modified).unwrap();
    let clash = folder.file("clash", "new");
    let into = folder.path().join("into");
    fs::create_dir(&into).unwrap();
    fs::write(into.join("clash"), "old").unwrap();
    let mut running = Running::for_test();
    let shared = Arc::clone(&running.shared);
    let canceller = thread::spawn(move || {
        let deadline = Instant::now() + ASK_WAIT;
        while shared.control.open().is_none() && Instant::now() < deadline {
            thread::yield_now();
        }
        shared.control.cancel();
    });

    let (result, _) = transfer_to(
        &mut running,
        &[(made, into.join("made")), (clash, into.join("clash"))],
        How::Copy,
    );
    canceller.join().unwrap();

    assert!(matches!(result, Err(Stop::Cancelled)));
    let copied = fs::metadata(into.join("made")).unwrap();
    assert_eq!(copied.permissions().mode() & 0o7777, 0o750);
    assert_eq!(copied.modified().unwrap(), modified);
}
