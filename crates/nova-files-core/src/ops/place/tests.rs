use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt as _;

use rustix::io::Errno;

use super::*;
use crate::ops::test_folder::TestFolder;

/// A folder anyone may read and enter, and its owner write.
const OPEN: u32 = 0o755;
/// A folder anyone may read and enter, and no one write.
const LOCKED: u32 = 0o555;

#[test]
fn renames_to_a_free_name() {
    let folder = TestFolder::new("place-free");
    let from = folder.file("a", "kept");
    let to = folder.path().join("b");

    rename_no_replace(&from, &to).unwrap();

    assert!(!from.exists());
    assert_eq!(fs::read_to_string(&to).unwrap(), "kept");
}

#[test]
fn never_replaces_a_taken_name() {
    let folder = TestFolder::new("place-taken");
    let from = folder.file("a", "new");
    let to = folder.file("b", "old");

    let error = rename_no_replace(&from, &to).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read_to_string(&from).unwrap(), "new");
    assert_eq!(fs::read_to_string(&to).unwrap(), "old");
}

#[test]
fn only_a_refused_flag_counts_as_unsupported() {
    assert!(refuses_flag(Errno::INVAL));
    assert!(refuses_flag(Errno::NOSYS));
    assert!(refuses_flag(Errno::OPNOTSUPP));
    assert!(!refuses_flag(Errno::EXIST));
    assert!(!refuses_flag(Errno::NOENT));
}

#[test]
fn the_link_fallback_moves_a_file_and_never_replaces() {
    let folder = TestFolder::new("place-link");
    let from = folder.file("a", "new");
    let taken = folder.file("b", "old");

    let error = link_or_check(&from, &taken).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read_to_string(&taken).unwrap(), "old");

    let free = folder.path().join("c");
    link_or_check(&from, &free).unwrap();
    assert!(!from.exists());
    assert_eq!(fs::read_to_string(&free).unwrap(), "new");
}

#[test]
fn a_folder_falls_back_to_check_then_rename() {
    let folder = TestFolder::new("place-check");
    let from = folder.path().join("dir");
    fs::create_dir(&from).unwrap();
    let taken = folder.file("b", "old");

    let error = link_or_check(&from, &taken).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert!(from.is_dir());

    let free = folder.path().join("moved");
    link_or_check(&from, &free).unwrap();
    assert!(!from.exists());
    assert!(free.is_dir());
}

#[test]
fn a_link_whose_source_cant_be_removed_is_taken_back() {
    let folder = TestFolder::new("place-unlink");
    // `inside` sits in a folder the test can't write, so linking it works
    // and removing it fails. Root may write anywhere.
    let from = folder.file("a", "new");
    let locked = folder.path().join("locked");
    fs::create_dir(&locked).unwrap();
    let inside = locked.join("a");
    fs::hard_link(&from, &inside).unwrap();
    set_writable(&locked, false);
    if fs::remove_file(&inside).is_ok() {
        set_writable(&locked, true);
        return;
    }
    let to = folder.path().join("b");

    let result = link_or_check(&inside, &to);
    set_writable(&locked, true);

    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert!(!to.exists());
    assert!(inside.exists());
}

#[test]
fn a_change_of_case_alone_is_no_clash() {
    let folder = TestFolder::new("place-case");
    // Two links to one file stand in for one entry found under two cases.
    let from = folder.file("photo", "kept");
    let to = folder.path().join("Photo");
    fs::hard_link(&from, &to).unwrap();
    let taken = |_: &Path, _: &Path| Err(Errno::EXIST);

    rename_or_fall_back(&from, &to, taken).unwrap();
    check_then_rename(&from, &to).unwrap();
    assert_eq!(fs::read_to_string(&to).unwrap(), "kept");
}

#[test]
fn only_the_same_entry_under_another_case_counts_as_a_case_change() {
    let folder = TestFolder::new("place-not-case");
    let from = folder.file("photo", "new");
    let other = folder.file("Photo", "old");
    let linked = folder.path().join("other");
    fs::hard_link(&from, &linked).unwrap();
    let odd = folder.path().join(OsStr::from_bytes(b"caf\xe9X"));
    let odd_lower = folder.path().join(OsStr::from_bytes(b"caf\xe9x"));
    fs::write(&odd, "").unwrap();
    fs::hard_link(&odd, &odd_lower).unwrap();
    let taken = |_: &Path, _: &Path| Err(Errno::EXIST);

    assert!(!is_case_change(&from, &other), "another file");
    assert!(!is_case_change(&from, &linked), "another name");
    assert!(
        !is_case_change(&from, &folder.path().join("PHOTO")),
        "nothing there"
    );
    assert!(!is_case_change(Path::new("/"), &from));
    assert!(is_case_change(&odd, &odd_lower), "bytes that aren't UTF-8");
    let accented = folder.file("Été", "");
    let accented_lower = folder.path().join("été");
    fs::hard_link(&accented, &accented_lower).unwrap();
    assert!(
        is_case_change(&accented, &accented_lower),
        "letters beyond ASCII"
    );
    let error = rename_or_fall_back(&from, &other, taken).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read_to_string(&other).unwrap(), "old");
}

#[test]
fn a_refused_flag_falls_back_and_any_other_error_stands() {
    let folder = TestFolder::new("place-fallback");
    let from = folder.file("a", "new");
    let free = folder.path().join("b");

    // Told the name is taken, though it is free: nothing is tried after.
    let error = rename_or_fall_back(&from, &free, |_, _| Err(Errno::EXIST)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert!(from.exists());
    assert!(!free.exists());

    // Told the flag is refused: the fallback renames.
    rename_or_fall_back(&from, &free, |_, _| Err(Errno::INVAL)).unwrap();
    assert!(!from.exists());
    assert_eq!(fs::read_to_string(&free).unwrap(), "new");
}

fn set_writable(folder: &Path, writable: bool) {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = if writable { OPEN } else { LOCKED };
    fs::set_permissions(folder, fs::Permissions::from_mode(mode)).unwrap();
}
