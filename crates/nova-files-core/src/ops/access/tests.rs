use std::fs::{self, Permissions};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _, symlink};

use rustix::process::geteuid;

use super::*;
use crate::ops::test_folder::TestFolder;

const FILE: u32 = 0o644;
const FOLDER: u32 = 0o755;
/// Group may write; others may not read.
const SHARE_FILES: ModeChange = ModeChange {
    set: 0o020,
    clear: 0o004,
};
/// Group may write; others may not enter.
const SHARE_FOLDERS: ModeChange = ModeChange {
    set: 0o020,
    clear: 0o005,
};

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().mode() & MODE_BITS
}

fn set_mode(path: &Path, mode: u32) {
    fs::set_permissions(path, Permissions::from_mode(mode)).unwrap();
}

fn share(reach: Reach) -> AccessChange {
    AccessChange {
        files: SHARE_FILES,
        folders: SHARE_FOLDERS,
        reach,
        ..AccessChange::default()
    }
}

fn run(paths: &[PathBuf], change: &AccessChange) -> Ended {
    change_access(&mut Running::for_test(), paths, change)
}

/// A folder `tree` holding a file, a folder with a file, and a link to a
/// file outside it; returns the tree and the file outside.
fn tree(folder: &TestFolder) -> (PathBuf, PathBuf) {
    let tree = folder.path().join("tree");
    fs::create_dir_all(tree.join("sub")).unwrap();
    let outside = folder.file("outside", "o");
    for file in [tree.join("a"), tree.join("sub/b")] {
        fs::write(&file, "x").unwrap();
        set_mode(&file, FILE);
    }
    for inner in [tree.clone(), tree.join("sub")] {
        set_mode(&inner, FOLDER);
    }
    set_mode(&outside, FILE);
    symlink(&outside, tree.join("link")).unwrap();
    (tree, outside)
}

#[test]
fn files_and_folders_inside_each_get_their_own_change_and_links_are_passed_over() {
    let folder = TestFolder::new("access-inside");
    let (tree, outside) = tree(&folder);

    let (outcome, changed) = run(std::slice::from_ref(&tree), &share(Reach::Inside));

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(
        changed,
        vec![folder.path().to_path_buf(), tree.clone(), tree.join("sub")]
    );
    assert_eq!(mode(&tree), 0o770);
    assert_eq!(mode(&tree.join("sub")), 0o770);
    assert_eq!(mode(&tree.join("a")), 0o660);
    assert_eq!(mode(&tree.join("sub/b")), 0o660);
    assert_eq!(mode(&outside), FILE);
}

#[test]
fn only_the_chosen_paths_change_unless_the_change_reaches_inside() {
    let folder = TestFolder::new("access-chosen");
    let (tree, _) = tree(&folder);

    let (outcome, _) = run(&[tree.clone(), tree.join("a")], &share(Reach::Chosen));

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(mode(&tree), 0o770);
    assert_eq!(mode(&tree.join("a")), 0o660);
    assert_eq!(mode(&tree.join("sub")), FOLDER);
    assert_eq!(mode(&tree.join("sub/b")), FILE);
}

#[test]
fn a_chosen_link_is_passed_over() {
    let folder = TestFolder::new("access-link");
    let (tree, outside) = tree(&folder);

    let (outcome, changed) = run(&[tree.join("link")], &share(Reach::Inside));

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(changed, Vec::<PathBuf>::new());
    assert_eq!(mode(&outside), FILE);
}

#[test]
fn a_folder_closed_to_its_owner_is_opened_up_and_then_gone_into() {
    let folder = TestFolder::new("access-closed");
    let (tree, _) = tree(&folder);
    set_mode(&tree, 0o000);
    let open_up = AccessChange {
        folders: ModeChange {
            set: 0o700,
            clear: 0,
        },
        files: ModeChange {
            set: 0o100,
            clear: 0,
        },
        reach: Reach::Inside,
        ..AccessChange::default()
    };

    let (outcome, _) = run(std::slice::from_ref(&tree), &open_up);

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(mode(&tree), 0o700);
    assert_eq!(mode(&tree.join("sub/b")), 0o744);
}

#[test]
fn nothing_to_change_changes_nothing_and_a_missing_path_is_passed_over() {
    let folder = TestFolder::new("access-nothing");
    let (tree, _) = tree(&folder);

    let (outcome, changed) = run(
        &[tree.join("a"), tree.join("gone")],
        &AccessChange::default(),
    );

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(changed, Vec::<PathBuf>::new());
    assert_eq!(mode(&tree.join("a")), FILE);
}

#[test]
fn a_path_that_is_not_absolute_changes_nothing() {
    let folder = TestFolder::new("access-relative");
    let (tree, _) = tree(&folder);

    let (outcome, _) = run(&[tree.join("a"), "a".into()], &share(Reach::Chosen));

    assert_eq!(
        outcome,
        Outcome::Failed {
            path: "a".into(),
            reason: Failure::InvalidName
        }
    );
    assert_eq!(mode(&tree.join("a")), FILE);
}

#[test]
fn a_cancelled_job_changes_nothing() {
    let folder = TestFolder::new("access-cancel");
    let (tree, _) = tree(&folder);
    let mut running = Running::for_test();
    running.shared.control.cancel();

    let (outcome, _) = change_access(&mut running, &[tree.join("a")], &share(Reach::Chosen));

    assert_eq!(outcome, Outcome::Cancelled);
    assert_eq!(mode(&tree.join("a")), FILE);
}

#[test]
fn a_new_owner_and_group_take_the_set_user_bit_away_unless_asked_for() {
    /// Runs as its owner, set-user.
    const SET_USER: u32 = 0o4755;
    let (owner, group) = (4242, 4343);
    if !geteuid().is_root() {
        return;
    }
    let folder = TestFolder::new("access-owner");
    let given = folder.file("given", "#!");
    let kept = folder.file("kept", "#!");
    set_mode(&given, SET_USER);
    set_mode(&kept, SET_USER);
    let give = AccessChange {
        owner: Some(owner),
        group: Some(group),
        ..AccessChange::default()
    };
    let keep = AccessChange {
        files: ModeChange {
            set: 0o4000,
            clear: 0,
        },
        ..give
    };

    let (outcome, changed) = run(std::slice::from_ref(&given), &give);
    run(std::slice::from_ref(&kept), &keep);

    let metadata = fs::metadata(&given).unwrap();
    assert_eq!(outcome, Outcome::Done);
    assert_eq!(changed, vec![folder.path().to_path_buf()]);
    assert_eq!((metadata.uid(), metadata.gid()), (owner, group));
    assert_eq!(mode(&given), 0o755);
    assert_eq!(mode(&kept), SET_USER);
}

#[test]
fn a_change_that_shuts_the_owner_out_does_what_is_inside_first() {
    let folder = TestFolder::new("access-shut");
    let (tree, _) = tree(&folder);
    let shut = AccessChange {
        folders: ModeChange {
            set: 0,
            clear: 0o700,
        },
        files: ModeChange {
            set: 0,
            clear: 0o200,
        },
        reach: Reach::Inside,
        ..AccessChange::default()
    };

    let (outcome, changed) = run(std::slice::from_ref(&tree), &shut);
    // Opened again to look inside.
    for inner in [tree.clone(), tree.join("sub")] {
        set_mode(&inner, FOLDER);
    }

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(
        changed,
        vec![folder.path().to_path_buf(), tree.clone(), tree.join("sub")]
    );
    assert_eq!(mode(&tree.join("a")), 0o444);
    assert_eq!(mode(&tree.join("sub/b")), 0o444);
}

#[test]
fn the_kernel_says_where_a_drive_is_mounted_and_older_kernels_go_by_the_drive() {
    let root = StatxAttributes::MOUNT_ROOT;
    let none = StatxAttributes::empty();
    let (one, other) = ((8, 1), (8, 2));

    assert!(is_mount_root(root, root, one, one));
    // A btrfs subvolume: another drive number, but no mount.
    assert!(!is_mount_root(root, none, other, one));
    assert!(is_mount_root(none, none, other, one));
    assert!(!is_mount_root(none, none, one, one));
}

/// Visits `path`, chosen, as `change` says; returns whether anything was
/// left to change last, once what it holds is done.
fn visit_chosen(path: &Path, change: &AccessChange) -> bool {
    let next = Next {
        path: path.to_path_buf(),
        at: Place::Chosen,
        inside: None,
    };
    let mut waiting = Vec::new();
    assert!(visit(&mut Running::for_test(), change, next, &mut waiting).is_ok());
    waiting
        .iter()
        .any(|task| matches!(task, Task::Close { .. }))
}

#[test]
fn only_a_folder_shut_to_its_owner_and_gone_into_is_changed_last() {
    let folder = TestFolder::new("access-last");
    let (tree, _) = tree(&folder);
    let shut = |reach| AccessChange {
        folders: ModeChange {
            set: 0,
            clear: 0o700,
        },
        reach,
        ..AccessChange::default()
    };

    let shut_later = visit_chosen(&tree, &shut(Reach::Inside));
    let left = mode(&tree);
    let shared_later = visit_chosen(&tree, &share(Reach::Inside));
    let shared = mode(&tree);
    set_mode(&tree, FOLDER);
    let alone_later = visit_chosen(&tree, &shut(Reach::Chosen));
    let alone = mode(&tree);
    set_mode(&tree, FOLDER);

    assert!(shut_later);
    assert_eq!(left, FOLDER);
    assert!(!shared_later);
    assert_eq!(shared, 0o770);
    assert!(!alone_later);
    assert_eq!(alone, 0o055);
    assert_eq!(mode(&tree.join("sub")), FOLDER);
}

#[test]
fn a_new_owner_alone_is_given_or_refused() {
    let folder = TestFolder::new("access-owner-alone");
    let file = folder.file("file", "x");
    let at = openat(CWD, &file, AT_PATH, Mode::empty()).unwrap();
    let give = AccessChange {
        owner: Some(4242),
        ..AccessChange::default()
    };

    let given = apply(&at, &give, ModeChange::default());

    if geteuid().is_root() {
        assert_eq!(given, Ok(true));
        assert_eq!(fs::metadata(&file).unwrap().uid(), 4242);
    } else {
        assert_eq!(given, Err(Errno::PERM));
    }
}

#[test]
fn only_a_missing_link_in_proc_means_no_proc() {
    assert_eq!(without_proc(Errno::NOENT), Errno::OPNOTSUPP);
    assert_eq!(without_proc(Errno::PERM), Errno::PERM);
}
