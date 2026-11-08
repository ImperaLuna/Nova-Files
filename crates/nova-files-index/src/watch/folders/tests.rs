//! The event rules, fed made-up events over real temp folders.

use super::*;
use rustix::fs::inotify::CreateFlags;

fn folders(root: &Path) -> Folders {
    let fd = inotify::init(CreateFlags::CLOEXEC.union(CreateFlags::NONBLOCK)).unwrap();
    Folders::new(fd, root.to_owned(), Box::new(|| false))
}

#[test]
fn an_overflow_asks_for_a_rescan_of_everything() {
    let mut folders = folders(Path::new("/home/me"));
    let overflow = Event {
        wd: -1,
        mask: ReadFlags::QUEUE_OVERFLOW,
        cookie: 0,
        name: None,
    };
    assert_eq!(
        folders.handle(&[overflow]),
        [Change::Rescan {
            folder: PathBuf::from("/home/me")
        }]
    );
}

#[test]
fn events_from_unknown_watches_are_passed_over() {
    let mut folders = folders(Path::new("/home/me"));
    let stray = Event {
        wd: 7,
        mask: ReadFlags::CREATE,
        cookie: 0,
        name: Some(OsString::from("a.txt")),
    };
    assert_eq!(folders.handle(&[stray]), []);
}

#[test]
fn a_watch_the_kernel_dropped_is_forgotten() {
    let root =
        std::env::temp_dir().join(format!("nova-files-watch-ignored-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let mut folders = folders(&root);
    assert_eq!(folders.watch(root.clone(), Zone::Home), None);
    let wd = folders.by_path[&root];
    let ignored = Event {
        wd,
        mask: ReadFlags::IGNORED,
        cookie: 0,
        name: None,
    };
    // Home itself went: what takes its place is read again.
    assert_eq!(
        folders.handle(&[ignored]),
        [Change::Rescan {
            folder: root.clone()
        }]
    );
    assert!(folders.by_wd.is_empty());
    assert!(folders.by_path.is_empty());
    fs::remove_dir_all(&root).ok();
}

#[test]
fn folders_not_read_by_the_crawl_are_not_watched() {
    use crate::mounts::MountCheck;
    struct DriveIsMounted(PathBuf);
    impl MountCheck for DriveIsMounted {
        fn is_mount_root(&self, folder: &Path) -> bool {
            folder == self.0
        }
    }
    let root = std::env::temp_dir().join(format!("nova-files-watch-drive-{}", std::process::id()));
    fs::create_dir_all(root.join("drive/inside")).unwrap();
    fs::create_dir_all(root.join("plain")).unwrap();
    let built = crawl::crawl_with(&root, DriveIsMounted(root.join("drive")), || false).unwrap();
    let saved = root.with_extension("index");
    built.save(&saved).unwrap();
    let index = Index::open(&saved).unwrap();
    let mut folders = folders(&root);
    assert_eq!(folders.watch_index(&index), []);
    let mut watched: Vec<&PathBuf> = folders.by_path.keys().collect();
    watched.sort();
    assert_eq!(watched, [&root, &root.join("plain")]);
    fs::remove_file(&saved).ok();
    fs::remove_dir_all(&root).ok();
}

/// A temp folder named after `test`, made afresh with `paths` in it
/// (a path ending in `/` is a folder), and watched.
fn watched_folder(test: &str, paths: &[&str], stopping: Stopping) -> (PathBuf, Folders) {
    let root = std::env::temp_dir().join(format!("nova-files-{test}-{}", std::process::id()));
    fs::remove_dir_all(&root).ok();
    fs::create_dir_all(&root).unwrap();
    for path in paths {
        if path.ends_with('/') {
            fs::create_dir_all(root.join(path)).unwrap();
        } else {
            fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            fs::write(root.join(path), b"").unwrap();
        }
    }
    let fd = inotify::init(CreateFlags::CLOEXEC.union(CreateFlags::NONBLOCK)).unwrap();
    let mut folders = Folders::new(fd, root.clone(), stopping);
    assert_eq!(folders.watch(root.clone(), Zone::Home), None);
    (root, folders)
}

fn event(folders: &Folders, folder: &Path, mask: ReadFlags, cookie: u32, name: &str) -> Event {
    Event {
        wd: folders.by_path[folder],
        mask,
        cookie,
        name: Some(OsString::from(name)),
    }
}

fn added_paths(changes: &[Change]) -> Vec<&Path> {
    changes
        .iter()
        .filter_map(|change| match change {
            Change::Added { path, .. } => Some(path.as_path()),
            _ => None,
        })
        .collect()
}

const DIR: ReadFlags = ReadFlags::ISDIR;

#[test]
fn a_new_tree_is_not_read_while_stopping() {
    let (root, mut folders) = watched_folder("stopping", &["new/inner/file"], Box::new(|| true));
    let created = event(&folders, &root, ReadFlags::CREATE.union(DIR), 0, "new");
    assert_eq!(folders.handle(&[created]), []);
    assert!(!folders.by_path.contains_key(&root.join("new")));
    fs::remove_dir_all(&root).ok();
}

#[test]
fn a_folder_made_and_moved_before_its_events_are_read_is_watched() {
    // `old` was made, filled and renamed `new` before any event was
    // read, so only `new` is on disk.
    let (root, mut folders) = watched_folder("made-moved", &["new/file"], Box::new(|| false));
    let changes = folders.handle(&[
        event(&folders, &root, ReadFlags::CREATE.union(DIR), 0, "old"),
        event(&folders, &root, ReadFlags::MOVED_FROM.union(DIR), 3, "old"),
        event(&folders, &root, ReadFlags::MOVED_TO.union(DIR), 3, "new"),
    ]);
    assert!(added_paths(&changes).contains(&root.join("new/file").as_path()));
    assert!(folders.by_path.contains_key(&root.join("new")));
    fs::remove_dir_all(&root).ok();
}

#[test]
fn a_file_moved_inside_home_is_added_at_its_new_path_too() {
    let (root, mut folders) = watched_folder("file-moved", &["b.txt"], Box::new(|| false));
    let changes = folders.handle(&[
        event(&folders, &root, ReadFlags::MOVED_FROM, 4, "a.txt"),
        event(&folders, &root, ReadFlags::MOVED_TO, 4, "b.txt"),
    ]);
    assert!(matches!(&changes[0], Change::Moved { to, .. } if to == &root.join("b.txt")));
    assert_eq!(added_paths(&changes), [root.join("b.txt")]);
    fs::remove_dir_all(&root).ok();
}

#[test]
fn a_watched_folder_moved_inside_home_keeps_its_watches() {
    let (root, mut folders) = watched_folder("folder-moved", &["new/file"], Box::new(|| false));
    // Watched as `old`, renamed `new` on disk before the events are read.
    let (old, new) = (root.join("old"), root.join("new"));
    fs::create_dir(&old).unwrap();
    let zone = policy::enters(&Zone::Home, OsStr::new("old")).unwrap();
    assert_eq!(folders.watch(old.clone(), zone), None);
    fs::remove_dir(&old).unwrap();
    let changes = folders.handle(&[
        event(&folders, &root, ReadFlags::MOVED_FROM.union(DIR), 6, "old"),
        event(&folders, &root, ReadFlags::MOVED_TO.union(DIR), 6, "new"),
    ]);
    assert_eq!(
        changes,
        [Change::Moved {
            from: old.clone(),
            to: new.clone()
        }]
    );
    assert!(folders.by_path.contains_key(&new));
    assert!(!folders.by_path.contains_key(&old));
    fs::remove_dir_all(&root).ok();
}

#[test]
fn a_folder_moved_over_another_keeps_its_watch() {
    let (root, mut folders) = watched_folder("over", &["x/", "y/"], Box::new(|| false));
    let (x, y) = (root.join("x"), root.join("y"));
    let zone = policy::enters(&Zone::Home, OsStr::new("x")).unwrap();
    assert_eq!(folders.watch(x.clone(), zone.clone()), None);
    assert_eq!(folders.watch(y.clone(), zone), None);
    let (moving, overwritten) = (folders.by_path[&x], folders.by_path[&y]);
    fs::rename(&x, &y).unwrap();
    folders.handle(&[
        event(&folders, &root, ReadFlags::MOVED_FROM.union(DIR), 7, "x"),
        event(&folders, &root, ReadFlags::MOVED_TO.union(DIR), 7, "y"),
        Event {
            wd: overwritten,
            mask: ReadFlags::IGNORED,
            cookie: 0,
            name: None,
        },
    ]);
    assert_eq!(folders.by_path.get(&y), Some(&moving));
    fs::remove_dir_all(&root).ok();
}

#[test]
fn a_dropped_watch_inside_home_asks_for_nothing() {
    let (root, mut folders) = watched_folder("dropped", &["a/"], Box::new(|| false));
    let a = root.join("a");
    let zone = policy::enters(&Zone::Home, OsStr::new("a")).unwrap();
    assert_eq!(folders.watch(a.clone(), zone), None);
    let ignored = Event {
        wd: folders.by_path[&a],
        mask: ReadFlags::IGNORED,
        cookie: 0,
        name: None,
    };
    assert_eq!(folders.handle(&[ignored]), []);
    assert!(!folders.by_path.contains_key(&a));
    fs::remove_dir_all(&root).ok();
}

#[test]
fn home_moved_away_asks_for_a_rescan() {
    let (root, mut folders) = watched_folder("home-moved", &["a/"], Box::new(|| false));
    let a = root.join("a");
    let zone = policy::enters(&Zone::Home, OsStr::new("a")).unwrap();
    assert_eq!(folders.watch(a.clone(), zone), None);
    let moved_self = |wd| Event {
        wd,
        mask: ReadFlags::MOVE_SELF,
        cookie: 0,
        name: None,
    };
    // Another folder's own move is seen from its parent.
    assert_eq!(folders.handle(&[moved_self(folders.by_path[&a])]), []);
    assert_eq!(
        folders.handle(&[moved_self(folders.by_path[&root])]),
        [Change::Rescan {
            folder: root.clone()
        }]
    );
    assert!(folders.by_path.is_empty());
    fs::remove_dir_all(&root).ok();
}

#[test]
fn a_folder_watched_again_at_its_new_path_leaves_the_old_one() {
    let (root, mut folders) = watched_folder("rewatched", &["a/"], Box::new(|| false));
    let (a, b) = (root.join("a"), root.join("b"));
    let zone = policy::enters(&Zone::Home, OsStr::new("a")).unwrap();
    assert_eq!(folders.watch(a.clone(), zone.clone()), None);
    let wd = folders.by_path[&a];
    // Moved away and back under another name before the events were read.
    fs::rename(&a, &b).unwrap();
    assert_eq!(folders.watch(b.clone(), zone), None);
    assert_eq!(folders.by_path.get(&b), Some(&wd));
    assert!(!folders.by_path.contains_key(&a));
    // Its old path's leaving doesn't take the watch with it.
    folders.handle(&[event(
        &folders,
        &root,
        ReadFlags::MOVED_FROM.union(DIR),
        12,
        "a",
    )]);
    assert_eq!(folders.by_path.get(&b), Some(&wd));
    fs::remove_dir_all(&root).ok();
}

#[test]
fn a_folder_that_left_its_parent_is_removed_before_the_parent_moves() {
    let (root, mut folders) = watched_folder("left-then-moved", &["a/sub/"], Box::new(|| false));
    let (a, sub, b) = (root.join("a"), root.join("a/sub"), root.join("b"));
    let zone = policy::enters(&Zone::Home, OsStr::new("a")).unwrap();
    assert_eq!(folders.watch(a.clone(), zone.clone()), None);
    let inner = policy::enters(&zone, OsStr::new("sub")).unwrap();
    assert_eq!(folders.watch(sub.clone(), inner), None);
    // `a/sub` leaves home, then `a` becomes `b`, before any event is read.
    let outside = std::env::temp_dir().join(format!("nova-files-left-{}", std::process::id()));
    fs::rename(&sub, &outside).unwrap();
    fs::rename(&a, &b).unwrap();
    let changes = folders.handle(&[
        event(&folders, &a, ReadFlags::MOVED_FROM.union(DIR), 13, "sub"),
        event(&folders, &root, ReadFlags::MOVED_FROM.union(DIR), 14, "a"),
        event(&folders, &root, ReadFlags::MOVED_TO.union(DIR), 14, "b"),
    ]);
    assert_eq!(
        changes,
        [
            Change::Removed { path: sub.clone() },
            Change::Moved {
                from: a,
                to: b.clone()
            }
        ]
    );
    assert!(folders.by_path.contains_key(&b));
    assert!(!folders.by_path.contains_key(&b.join("sub")));
    assert!(!folders.by_path.contains_key(&sub));
    fs::remove_dir_all(&outside).ok();
    fs::remove_dir_all(&root).ok();
}

#[test]
fn swapped_files_are_both_added_as_they_are() {
    let (root, mut folders) = watched_folder("swap-files", &["a.txt", "b.txt"], Box::new(|| false));
    let (a, b) = (root.join("a.txt"), root.join("b.txt"));
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        &a,
        rustix::fs::CWD,
        &b,
        rustix::fs::RenameFlags::EXCHANGE,
    )
    .unwrap();
    let changes = folders.handle(&[
        event(&folders, &root, ReadFlags::MOVED_FROM, 15, "a.txt"),
        event(&folders, &root, ReadFlags::MOVED_TO, 15, "b.txt"),
        event(&folders, &root, ReadFlags::MOVED_FROM, 16, "b.txt"),
        event(&folders, &root, ReadFlags::MOVED_TO, 16, "a.txt"),
    ]);
    assert!(
        !changes
            .iter()
            .any(|change| matches!(change, Change::Moved { .. })),
        "{changes:?}"
    );
    let mut added = added_paths(&changes);
    added.sort();
    added.dedup();
    assert_eq!(added, [a.as_path(), &b]);
    fs::remove_dir_all(&root).ok();
}

#[test]
fn a_file_and_a_folder_swapped_are_both_read_again() {
    let (root, mut folders) =
        watched_folder("swap-mixed", &["a.txt", "b/inner.txt"], Box::new(|| false));
    let (a, b) = (root.join("a.txt"), root.join("b"));
    let zone = policy::enters(&Zone::Home, OsStr::new("b")).unwrap();
    assert_eq!(folders.watch(b.clone(), zone), None);
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        &a,
        rustix::fs::CWD,
        &b,
        rustix::fs::RenameFlags::EXCHANGE,
    )
    .unwrap();
    // The file moves first, then the folder.
    let changes = folders.handle(&[
        event(&folders, &root, ReadFlags::MOVED_FROM, 18, "a.txt"),
        event(&folders, &root, ReadFlags::MOVED_TO, 18, "b"),
        event(&folders, &root, ReadFlags::MOVED_FROM.union(DIR), 19, "b"),
        event(&folders, &root, ReadFlags::MOVED_TO.union(DIR), 19, "a.txt"),
    ]);
    // What is logged last for each path is what stands there now.
    let last = |path: &Path| {
        changes
            .iter()
            .rev()
            .find(|change| match change {
                Change::Added { path: at, .. } | Change::Removed { path: at } => at == path,
                _ => false,
            })
            .cloned()
    };
    assert!(matches!(last(&b), Some(Change::Added { details, .. }) if details.kind == Kind::File));
    assert!(
        matches!(last(&a), Some(Change::Added { details, .. }) if details.kind == Kind::Folder)
    );
    assert!(matches!(
        last(&a.join("inner.txt")),
        Some(Change::Added { .. })
    ));
    let wd = folders.by_path[&a];
    assert_eq!(inotify::add_watch(&folders.inotify, &a, WATCHED), Ok(wd));
    assert!(!folders.by_path.contains_key(&b));
    fs::remove_dir_all(&root).ok();
}

#[test]
fn a_hidden_folder_swapped_with_a_shown_one_is_read_as_it_is_now() {
    let (root, mut folders) = watched_folder(
        "swap-hidden",
        &[".hidden/secret.txt", "shown/old.txt"],
        Box::new(|| false),
    );
    let (hidden, shown) = (root.join(".hidden"), root.join("shown"));
    let zone = policy::enters(&Zone::Home, OsStr::new("shown")).unwrap();
    assert_eq!(folders.watch(shown.clone(), zone), None);
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        &hidden,
        rustix::fs::CWD,
        &shown,
        rustix::fs::RenameFlags::EXCHANGE,
    )
    .unwrap();
    let changes = folders.handle(&[
        event(
            &folders,
            &root,
            ReadFlags::MOVED_FROM.union(DIR),
            20,
            ".hidden",
        ),
        event(&folders, &root, ReadFlags::MOVED_TO.union(DIR), 20, "shown"),
        event(
            &folders,
            &root,
            ReadFlags::MOVED_FROM.union(DIR),
            21,
            "shown",
        ),
        event(
            &folders,
            &root,
            ReadFlags::MOVED_TO.union(DIR),
            21,
            ".hidden",
        ),
    ]);
    // What was hidden is now `shown`, and indexed.
    let last = changes.iter().rev().find(|change| match change {
        Change::Added { path, .. } | Change::Removed { path } => path == &shown,
        _ => false,
    });
    assert!(matches!(last, Some(Change::Added { .. })), "{changes:?}");
    assert!(added_paths(&changes).contains(&shown.join("secret.txt").as_path()));
    assert!(
        !added_paths(&changes)
            .iter()
            .any(|path| path.starts_with(&hidden))
    );
    let wd = folders.by_path[&shown];
    assert_eq!(
        inotify::add_watch(&folders.inotify, &shown, WATCHED),
        Ok(wd)
    );
    assert!(!folders.by_path.contains_key(&hidden));
    fs::remove_dir_all(&root).ok();
}

#[test]
fn a_profile_moved_over_a_folder_removes_it() {
    let (root, mut folders) = watched_folder(
        "profile-over",
        &["dest/", ".app/profiles.ini"],
        Box::new(|| false),
    );
    let dest = root.join("dest");
    fs::rename(root.join(".app"), &dest).unwrap();
    let changes = folders.handle(&[
        event(
            &folders,
            &root,
            ReadFlags::MOVED_FROM.union(DIR),
            17,
            ".app",
        ),
        event(&folders, &root, ReadFlags::MOVED_TO.union(DIR), 17, "dest"),
    ]);
    assert_eq!(changes, [Change::Removed { path: dest.clone() }]);
    assert!(!folders.by_path.contains_key(&dest));
    fs::remove_dir_all(&root).ok();
}

#[test]
fn swapped_folders_are_both_read_again() {
    let (root, mut folders) =
        watched_folder("swap", &["a/one.txt", "b/two.txt"], Box::new(|| false));
    let (a, b) = (root.join("a"), root.join("b"));
    let zone = policy::enters(&Zone::Home, OsStr::new("a")).unwrap();
    assert_eq!(folders.watch(a.clone(), zone.clone()), None);
    assert_eq!(folders.watch(b.clone(), zone), None);
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        &a,
        rustix::fs::CWD,
        &b,
        rustix::fs::RenameFlags::EXCHANGE,
    )
    .unwrap();
    let changes = folders.handle(&[
        event(&folders, &root, ReadFlags::MOVED_FROM.union(DIR), 10, "a"),
        event(&folders, &root, ReadFlags::MOVED_TO.union(DIR), 10, "b"),
        event(&folders, &root, ReadFlags::MOVED_FROM.union(DIR), 11, "b"),
        event(&folders, &root, ReadFlags::MOVED_TO.union(DIR), 11, "a"),
    ]);
    let mut added = added_paths(&changes);
    added.sort();
    added.dedup();
    assert_eq!(
        added,
        [a.as_path(), &a.join("two.txt"), &b, &b.join("one.txt")]
    );
    // Each path's watch is the one the kernel has on the folder there now.
    for path in [&a, &b] {
        let wd = folders.by_path[path];
        assert_eq!(inotify::add_watch(&folders.inotify, path, WATCHED), Ok(wd));
        assert_eq!(&folders.by_wd[&wd].path, path);
    }
    fs::remove_dir_all(&root).ok();
}

#[test]
fn a_folder_moved_twice_before_its_events_are_read_follows_both() {
    let (root, mut folders) = watched_folder("twice", &["a/"], Box::new(|| false));
    let a = root.join("a");
    // Watched as `a`; renamed `b`, then `.hidden`, before any event
    // was read, so `b` is not on disk.
    let zone = policy::enters(&Zone::Home, OsStr::new("a")).unwrap();
    assert_eq!(folders.watch(a.clone(), zone), None);
    fs::rename(&a, root.join(".hidden")).unwrap();
    let changes = folders.handle(&[
        event(&folders, &root, ReadFlags::MOVED_FROM.union(DIR), 8, "a"),
        event(&folders, &root, ReadFlags::MOVED_TO.union(DIR), 8, "b"),
        event(&folders, &root, ReadFlags::MOVED_FROM.union(DIR), 9, "b"),
        event(
            &folders,
            &root,
            ReadFlags::MOVED_TO.union(DIR),
            9,
            ".hidden",
        ),
    ]);
    let b = root.join("b");
    assert_eq!(
        changes,
        [
            Change::Moved {
                from: a.clone(),
                to: b.clone()
            },
            Change::Removed { path: b.clone() }
        ]
    );
    assert!(!folders.by_path.contains_key(&a) && !folders.by_path.contains_key(&b));
    fs::remove_dir_all(&root).ok();
}

#[test]
fn another_drive_is_added_but_not_entered() {
    let mut folders = folders(Path::new("/"));
    let mut changes = Vec::new();
    let zone = Zone::Open {
        allowed_place: false,
    };
    folders.add(
        &zone,
        OsStr::new("proc"),
        PathBuf::from("/proc"),
        &mut changes,
    );
    let [Change::Added { path, details }] = changes.as_slice() else {
        panic!("{changes:?}");
    };
    assert_eq!(path, Path::new("/proc"));
    assert!(details.flags.contains(Flags::OTHER_DRIVE));
    assert!(folders.by_path.is_empty());
}

#[test]
fn a_folder_made_where_one_moved_away_replaces_it() {
    let (root, mut folders) = watched_folder("reused", &["same/file"], Box::new(|| false));
    let changes = folders.handle(&[
        event(&folders, &root, ReadFlags::MOVED_FROM.union(DIR), 5, "same"),
        event(&folders, &root, ReadFlags::CREATE.union(DIR), 0, "same"),
    ]);
    let same = root.join("same");
    assert_eq!(changes[0], Change::Removed { path: same.clone() });
    assert!(added_paths(&changes).contains(&same.join("file").as_path()));
    assert!(folders.by_path.contains_key(&same));
    fs::remove_dir_all(&root).ok();
}

#[test]
fn a_folder_that_can_be_read_again_is_read() {
    // Unreadable at the crawl, so not watched; its mode changed.
    let (root, mut folders) = watched_folder("readable", &["locked/file"], Box::new(|| false));
    let changes = folders.handle(&[event(
        &folders,
        &root,
        ReadFlags::ATTRIB.union(DIR),
        0,
        "locked",
    )]);
    assert!(added_paths(&changes).contains(&root.join("locked/file").as_path()));
    assert!(folders.by_path.contains_key(&root.join("locked")));
    // Once watched, a change of mode is only a change.
    let again = folders.handle(&[event(
        &folders,
        &root,
        ReadFlags::ATTRIB.union(DIR),
        0,
        "locked",
    )]);
    assert!(matches!(again.as_slice(), [Change::Changed { .. }]));
    fs::remove_dir_all(&root).ok();
}

#[test]
fn a_folder_that_cant_be_watched_gets_a_rescan() {
    let mut folders = folders(Path::new("/"));
    let missing = PathBuf::from("/nowhere/at/all");
    assert_eq!(folders.watch(missing, Zone::Home), None);
    // A file isn't a folder: watching it is refused as gone.
    let file = std::env::temp_dir().join(format!("nova-files-watch-file-{}", std::process::id()));
    fs::write(&file, b"").unwrap();
    assert_eq!(folders.watch(file.clone(), Zone::Home), None);
    fs::remove_file(&file).ok();
}
