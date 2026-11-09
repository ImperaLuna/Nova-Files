use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::symlink;

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

#[test]
fn a_file_saved_anew_after_the_link_is_never_removed() {
    let folder = TestFolder::new("place-saved-anew");
    let from = folder.file("a", "old");
    let to = folder.path().join("b");
    fs::hard_link(&from, &to).unwrap();
    // An editor saves a new `a` by renaming over it.
    let saved = folder.file(".a.tmp", "new");
    fs::rename(&saved, &from).unwrap();

    let error = unlink_or_take_back(&from, &to).unwrap_err();

    assert_eq!(error.raw_os_error(), Some(Errno::AGAIN.raw_os_error()));
    assert_eq!(fs::read_to_string(&from).unwrap(), "new");
    assert!(!to.exists(), "the link is taken back");
}

#[test]
fn the_check_fallback_moves_each_kind_and_never_replaces() {
    let folder = TestFolder::new("place-reserve");
    let file = folder.file("f", "file");
    let link = folder.path().join("l");
    symlink("f", &link).unwrap();
    let taken = folder.file("taken", "old");

    for from in [&file, &link] {
        let error = check_then_rename(from, &taken).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    }
    assert_eq!(fs::read_to_string(&taken).unwrap(), "old");

    check_then_rename(&link, &folder.path().join("l2")).unwrap();
    assert_eq!(
        fs::read_link(folder.path().join("l2")).unwrap(),
        Path::new("f")
    );
    check_then_rename(&file, &folder.path().join("f2")).unwrap();
    assert_eq!(
        fs::read_to_string(folder.path().join("f2")).unwrap(),
        "file"
    );
    assert!(!file.exists() && link.symlink_metadata().is_err());
}

#[test]
fn a_failed_rename_gives_the_name_back() {
    let folder = TestFolder::new("place-reserve-fail");
    let from = folder.path().join("dir");
    fs::create_dir_all(from.join("inside")).unwrap();
    // A folder can't go into itself: the rename fails after the name is
    // taken.
    let to = from.join("inside/dir");

    assert!(check_then_rename(&from, &to).is_err());
    assert!(!to.exists());
    let gone = folder.path().join("gone");
    assert_eq!(
        check_then_rename(&gone, &folder.path().join("x"))
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );
    assert!(!folder.path().join("x").exists());
}

#[test]
fn a_replace_swaps_then_removes_the_old() {
    let folder = TestFolder::new("place-replace");
    let new = folder.file(".part", "new");
    let old = folder.file("a", "old");
    let backup = folder.path().join(".backup");

    replace(&new, &old, &backup).unwrap();

    assert_eq!(fs::read_to_string(&old).unwrap(), "new");
    assert!(!new.exists());
    assert!(!backup.exists());
}

#[test]
fn without_a_swap_the_old_steps_aside_and_goes_last() {
    let folder = TestFolder::new("place-backup");
    let new = folder.file(".part", "new");
    let old = folder.file("a", "old");
    let backup = folder.path().join(".backup");
    let refused = |_: &Path, _: &Path| Err(Errno::INVAL);

    replace_or_fall_back(&new, &old, &backup, refused).unwrap();

    assert_eq!(fs::read_to_string(&old).unwrap(), "new");
    assert!(!new.exists());
    assert!(!backup.exists());
}

#[test]
fn a_failed_replace_puts_the_old_back() {
    let folder = TestFolder::new("place-restore");
    let gone = folder.path().join(".part");
    let old = folder.file("a", "old");
    let backup = folder.path().join(".backup");
    let refused = |_: &Path, _: &Path| Err(Errno::INVAL);

    let error = replace_or_fall_back(&gone, &old, &backup, refused).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert_eq!(fs::read_to_string(&old).unwrap(), "old");
    assert!(!backup.exists());

    let failed = |_: &Path, _: &Path| Err(Errno::NOENT);
    let error = replace_or_fall_back(&gone, &old, &backup, failed).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert_eq!(fs::read_to_string(&old).unwrap(), "old");
}

#[test]
fn a_swap_that_fails_for_any_other_reason_replaces_nothing() {
    let folder = TestFolder::new("place-swap-fails");
    let new = folder.file(".part", "new");
    let old = folder.file("a", "old");
    let backup = folder.path().join(".backup");
    let elsewhere = |_: &Path, _: &Path| Err(Errno::XDEV);

    let error = replace_or_fall_back(&new, &old, &backup, elsewhere).unwrap_err();

    assert_eq!(error.raw_os_error(), Some(Errno::XDEV.raw_os_error()));
    assert_eq!(fs::read_to_string(&old).unwrap(), "old");
    assert_eq!(fs::read_to_string(&new).unwrap(), "new");
}
