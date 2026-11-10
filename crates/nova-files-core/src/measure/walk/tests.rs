use std::fs::{self, File};
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use nova_files_api::Size;

use super::*;
use crate::ops::test_folder::TestFolder;

/// No permissions at all.
const NO_ACCESS: u32 = 0o000;
/// Owner may read, write and enter.
const OWNER_ALL: u32 = 0o700;

fn write(path: &Path, bytes: usize) {
    fs::write(path, vec![b'x'; bytes]).unwrap();
}

/// The longest a walk in these tests may take: a walk that never ends
/// fails the test instead of hanging it.
const WALK_DEADLINE: Duration = Duration::from_secs(10);

/// Walks `roots` on a thread of its own, failing the test after
/// [`WALK_DEADLINE`].
fn walk_within(
    roots: &[Root],
    tally: &Tally,
    stopped: &(dyn Fn() -> bool + Sync),
    progress: &(dyn Fn() + Sync),
) {
    let (done, finished) = mpsc::channel();
    thread::scope(|scope| {
        scope.spawn(|| {
            walk(roots, tally, stopped, progress);
            done.send(()).ok();
        });
        if finished.recv_timeout(WALK_DEADLINE).is_err() {
            // The scope would wait for the stuck walk forever: end the
            // whole test process, which fails the test.
            eprintln!("the walk never ended");
            std::process::abort();
        }
    });
}

fn measure(roots: &[Root], slots: usize) -> Tally {
    let tally = Tally::new(slots);
    walk_within(roots, &tally, &|| false, &|| ());
    tally
}

fn one(path: &Path) -> Size {
    measure(
        &[Root {
            path: path.to_owned(),
            slot: 0,
            listed_in: None,
        }],
        1,
    )
    .total()
}

/// What `du` says `path` takes on the drive, by the same `statx` call.
fn blocks(path: &Path) -> u64 {
    statx(CWD, path, AtFlags::SYMLINK_NOFOLLOW, WANTED)
        .unwrap()
        .stx_blocks
        * BLOCK
}

#[test]
fn a_tree_counts_every_file_and_folder_at_every_depth() {
    let folder = TestFolder::new("walk-tree");
    let root = folder.path();
    fs::create_dir_all(root.join("a/b/c")).unwrap();
    write(&root.join("top"), 10);
    write(&root.join("a/one"), 100);
    write(&root.join("a/b/c/deep"), 1000);
    let on_disk: u64 = ["", "a", "a/b", "a/b/c", "top", "a/one", "a/b/c/deep"]
        .iter()
        .map(|part| blocks(&root.join(part)))
        .sum();

    let size = one(root);

    assert_eq!(
        size,
        Size {
            bytes: 1110,
            on_disk,
            files: 3,
            folders: 4,
            unreadable: 0,
        }
    );
}

#[test]
fn a_link_counts_as_itself_and_its_target_is_never_entered() {
    let folder = TestFolder::new("walk-link");
    let root = folder.path();
    let outside = TestFolder::new("walk-link-outside");
    write(&outside.path().join("big"), 5000);
    symlink(outside.path(), root.join("to-outside")).unwrap();

    let size = one(root);
    let link_length = u64::try_from(outside.path().as_os_str().len()).unwrap();

    assert_eq!(size.files, 1);
    assert_eq!(size.folders, 1);
    assert_eq!(size.bytes, link_length);
}

#[test]
fn a_root_that_is_a_link_is_not_followed() {
    let folder = TestFolder::new("walk-root-link");
    write(&folder.path().join("big"), 5000);
    let link = folder.path().join("link");
    symlink(folder.path().join("big"), &link).unwrap();

    let size = one(&link);

    assert_eq!(size.files, 1);
    assert!(size.bytes < 5000, "{size:?}");
}

#[test]
fn hard_links_count_once_however_many_roots_reach_them() {
    let folder = TestFolder::new("walk-hard-links");
    let root = folder.path();
    fs::create_dir(root.join("a")).unwrap();
    write(&root.join("a/file"), 700);
    fs::hard_link(root.join("a/file"), root.join("again")).unwrap();
    fs::hard_link(root.join("a/file"), root.join("a/thrice")).unwrap();
    let roots = [
        Root {
            path: root.join("a"),
            slot: 0,
            listed_in: None,
        },
        Root {
            path: root.join("again"),
            slot: 1,
            listed_in: None,
        },
    ];

    let tally = measure(&roots, 2);

    assert_eq!(tally.total().files, 1);
    assert_eq!(tally.total().bytes, 700);
}

#[test]
fn a_big_flat_folder_reports_as_it_goes() {
    let folder = TestFolder::new("walk-flat");
    let root = folder.path();
    // Two dot entries, then enough files for two reports inside.
    for name in 0..REPORT_EVERY * 2 + 10 {
        File::create(root.join(name.to_string())).unwrap();
    }
    let reports = AtomicUsize::new(0);
    let tally = Tally::new(1);

    walk_within(
        &[Root {
            path: root.to_owned(),
            slot: 0,
            listed_in: None,
        }],
        &tally,
        &|| false,
        &|| {
            reports.fetch_add(1, Ordering::SeqCst);
        },
    );

    // Two inside the folder, one once it is read.
    assert_eq!(reports.load(Ordering::SeqCst), 3);
}

#[test]
fn each_root_adds_to_its_own_slot() {
    let folder = TestFolder::new("walk-slots");
    let root = folder.path();
    fs::create_dir(root.join("a")).unwrap();
    write(&root.join("a/file"), 3);
    write(&root.join("b"), 4);
    let roots = [
        Root {
            path: root.join("a"),
            slot: 0,
            listed_in: None,
        },
        Root {
            path: root.join("b"),
            slot: 1,
            listed_in: None,
        },
    ];

    let tally = measure(&roots, 2);
    let mut slots = tally.take_changed();
    slots.sort_by_key(|(slot, _)| *slot);

    assert_eq!(slots.len(), 2);
    assert_eq!((slots[0].1.bytes, slots[0].1.folders), (3, 1));
    assert_eq!((slots[1].1.bytes, slots[1].1.files), (4, 1));
}

#[test]
fn a_missing_root_counts_as_unreadable() {
    let folder = TestFolder::new("walk-missing");

    let size = one(&folder.path().join("gone"));

    assert_eq!(
        size,
        Size {
            unreadable: 1,
            ..Size::default()
        }
    );
}

#[test]
fn a_folder_that_cannot_be_read_counts_as_unreadable_and_the_rest_still_adds_up() {
    let folder = TestFolder::new("walk-locked");
    let root = folder.path();
    fs::create_dir(root.join("locked")).unwrap();
    write(&root.join("locked/hidden"), 50);
    write(&root.join("open"), 5);
    fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(NO_ACCESS)).unwrap();
    // Root reads any folder; the test means nothing then.
    let readable = fs::read_dir(root.join("locked")).is_ok();

    let size = one(root);

    fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(OWNER_ALL)).unwrap();
    if readable {
        return;
    }
    assert_eq!(size.unreadable, 1);
    assert_eq!(size.bytes, 5);
    assert_eq!(size.folders, 2);
}

#[test]
fn a_walk_stopped_before_it_starts_reads_nothing() {
    let folder = TestFolder::new("walk-stopped");
    write(&folder.path().join("file"), 9);
    let tally = Tally::new(1);
    let roots = [Root {
        path: folder.path().to_owned(),
        slot: 0,
        listed_in: None,
    }];

    walk_within(&roots, &tally, &|| true, &|| ());

    assert_eq!(tally.total(), Size::default());
}

#[test]
fn a_walk_stopped_midway_reads_no_more_folders() {
    let folder = TestFolder::new("walk-stop-midway");
    let root = folder.path();
    fs::create_dir_all(root.join("a/b")).unwrap();
    write(&root.join("a/file"), 1);
    write(&root.join("a/b/file"), 1);
    let tally = Tally::new(1);
    let roots = [Root {
        path: root.to_owned(),
        slot: 0,
        listed_in: None,
    }];

    // Stops once the root's folder "a" is counted, before it is queued.
    walk_within(&roots, &tally, &|| tally.total().folders >= 2, &|| ());

    assert_eq!(tally.total().files, 0);
    assert_eq!(tally.total().folders, 2);
}

#[test]
fn every_folder_reports_when_it_is_done() {
    let folder = TestFolder::new("walk-reports");
    fs::create_dir_all(folder.path().join("a/b")).unwrap();
    let read = AtomicUsize::new(0);
    let roots = [Root {
        path: folder.path().to_owned(),
        slot: 0,
        listed_in: None,
    }];

    walk_within(&roots, &Tally::new(1), &|| false, &|| {
        read.fetch_add(1, Ordering::SeqCst);
    });

    assert_eq!(read.load(Ordering::SeqCst), 3);
}

/// A drive mounted inside the folder counts as a folder, and nothing
/// inside it counts. Uses `/dev`, which has `/dev/shm` mounted inside it
/// on any Linux this runs on; skipped where it doesn't.
#[test]
fn a_drive_mounted_inside_is_not_entered() {
    let dev = Path::new("/dev");
    let shm = dev.join("shm");
    let device = |path: &Path| {
        statx(CWD, path, AtFlags::SYMLINK_NOFOLLOW, WANTED)
            .map(|stat| (stat.stx_dev_major, stat.stx_dev_minor))
    };
    let (Ok(outer), Ok(inner)) = (device(dev), device(&shm)) else {
        return;
    };
    if outer == inner {
        return;
    }
    let big = shm.join(format!("nova-files-walk-mount-{}", std::process::id()));
    let Ok(file) = File::create(&big) else {
        return;
    };
    // Sparse: takes no space, but its length counts if it is reached.
    let length = 1 << 50;
    file.set_len(length).unwrap();

    let inside = one(&shm);
    let around = one(dev);

    fs::remove_file(&big).ok();
    assert!(inside.bytes >= length, "{inside:?}");
    assert!(around.bytes < length, "{around:?}");
}

#[test]
fn entries_that_cannot_be_looked_at_count_as_unreadable() {
    /// May list the folder, but not look at what is in it.
    const LIST_ONLY: u32 = 0o444;
    let folder = TestFolder::new("walk-list-only");
    let root = folder.path();
    fs::create_dir(root.join("listed")).unwrap();
    write(&root.join("listed/one"), 10);
    write(&root.join("listed/two"), 10);
    fs::set_permissions(root.join("listed"), fs::Permissions::from_mode(LIST_ONLY)).unwrap();
    // Root looks at anything; the test means nothing then.
    let blocked = fs::symlink_metadata(root.join("listed/one")).is_err();

    let size = one(root);

    fs::set_permissions(root.join("listed"), fs::Permissions::from_mode(OWNER_ALL)).unwrap();
    if !blocked {
        return;
    }
    assert_eq!((size.unreadable, size.files, size.bytes), (2, 0, 0));
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

#[test]
fn a_listed_entry_that_vanished_is_left_out_and_a_named_path_is_not() {
    let folder = TestFolder::new("walk-vanished");
    let gone = folder.path().join("gone");
    let device = device_of(folder.path());
    let roots = [
        Root {
            path: gone.clone(),
            slot: 0,
            listed_in: device,
        },
        Root {
            path: gone,
            slot: 1,
            listed_in: None,
        },
    ];

    let tally = measure(&roots, 2);
    let slots = tally.take_changed();

    assert!(device.is_some());
    assert_eq!(slots[0].1, Size::default());
    assert_eq!(slots[1].1.unreadable, 1);
}
