use super::*;
use crate::ops::TestFolder;
use crate::trash::{Trashes, info_text};
use std::time::{Duration, UNIX_EPOCH};

/// A trash at `folder/name` with `files/` and `info/`, for a drive
/// topped at `top` when given.
fn trash(folder: &TestFolder, name: &str, top: Option<&Path>) -> TrashFolder {
    let root = folder.path().join(name);
    fs::create_dir_all(root.join("files")).unwrap();
    fs::create_dir_all(root.join("info")).unwrap();
    TrashFolder {
        root,
        topdir: top.map(Path::to_path_buf),
    }
}

fn put(trash: &TrashFolder, stored: &str, info: &[u8]) -> PathBuf {
    let item = trash.files().join(stored);
    fs::write(&item, "trashed").unwrap();
    fs::write(info_of(trash, OsStr::new(stored)), info).unwrap();
    item
}

#[test]
fn lists_what_a_trash_holds_from_its_info_files() {
    let folder = TestFolder::new("read-list");
    let home = trash(&folder, "Trash", None);
    let when = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let file = put(
        &home,
        "a.txt",
        &info_text(Path::new("/home/me/a b.txt"), when),
    );
    fs::create_dir(home.files().join("pics")).unwrap();
    fs::write(
        info_of(&home, OsStr::new("pics")),
        info_text(Path::new("/home/me/pics"), when),
    )
    .unwrap();

    let mut listed = list(&home);
    listed.sort_by(|a, b| a.item.cmp(&b.item));

    assert_eq!(
        listed,
        vec![
            Trashed {
                item: file,
                original: "/home/me/a b.txt".into(),
                deleted: Some(when),
                kind: EntryKind::File,
            },
            Trashed {
                item: home.files().join("pics"),
                original: "/home/me/pics".into(),
                deleted: Some(when),
                kind: EntryKind::Folder,
            },
        ]
    );
}

#[test]
fn a_drive_trash_paths_are_from_the_drive_top() {
    let folder = TestFolder::new("read-drive");
    let top = folder.path().join("drive");
    let drive = trash(&folder, "drive/.Trash-1000", Some(&top));
    put(&drive, "a", b"[Trash Info]\nPath=docs/a\n");
    put(&drive, "up", b"[Trash Info]\nPath=../etc/passwd\n");
    put(&drive, "dot", b"[Trash Info]\nPath=./a\n");

    let listed = list(&drive);

    assert_eq!(listed.len(), 1, "never a path out of the drive: {listed:?}");
    assert_eq!(listed[0].original, top.join("docs/a"));
}

#[test]
fn a_home_trash_path_must_be_whole() {
    let folder = TestFolder::new("read-relative");
    let home = trash(&folder, "Trash", None);
    put(&home, "a", b"[Trash Info]\nPath=docs/a\n");

    assert_eq!(list(&home), vec![]);
}

#[test]
fn an_unreadable_date_is_unknown_and_the_entry_still_listed() {
    let folder = TestFolder::new("read-date");
    let home = trash(&folder, "Trash", None);
    put(
        &home,
        "a",
        b"[Trash Info]\nPath=/a\nDeletionDate=yesterday-ish\n",
    );
    put(&home, "b", b"[Trash Info]\nPath=/b\n");

    let listed = list(&home);

    assert_eq!(listed.len(), 2);
    assert!(listed.iter().all(|entry| entry.deleted.is_none()));
}

#[test]
fn reads_other_apps_info_files() {
    let folder = TestFolder::new("read-others");
    let home = trash(&folder, "Trash", None);
    let text = b"# made elsewhere\r\n\r\n[Trash Info]\r\nPath=/caf%C3%A9%20menu\r\n\
DeletionDate=2024-03-01T10:20:30\r\n[Other]\r\nPath=/not-this\r\n";
    put(&home, "a", text);

    let listed = list(&home);

    assert_eq!(listed[0].original, PathBuf::from("/café menu"));
    let naive = NaiveDateTime::parse_from_str("2024-03-01T10:20:30", DATE_FORMAT).unwrap();
    let local = Local.from_local_datetime(&naive).earliest().unwrap();
    assert_eq!(listed[0].deleted, Some(SystemTime::from(local)));
}

#[test]
fn what_isnt_an_info_file_is_left_out() {
    let folder = TestFolder::new("read-bad");
    let home = trash(&folder, "Trash", None);
    put(&home, "no-group", b"Path=/a\n");
    put(&home, "late-group", b"Path=/a\n[Trash Info]\n");
    put(
        &home,
        "no-path",
        b"[Trash Info]\nDeletionDate=2024-03-01T10:20:30\n",
    );
    put(&home, "empty-path", b"[Trash Info]\nPath=\n");
    put(&home, "bad-escape", b"[Trash Info]\nPath=/a%zz\n");
    put(&home, "path-after", b"[Trash Info]\n[Other]\nPath=/a\n");
    // An info file without its entry, and an entry without one.
    fs::write(
        info_of(&home, OsStr::new("gone")),
        b"[Trash Info]\nPath=/g\n",
    )
    .unwrap();
    fs::write(home.files().join("lone"), "x").unwrap();
    fs::write(home.info().join(".trashinfo"), b"[Trash Info]\nPath=/n\n").unwrap();
    fs::write(home.info().join("notes"), b"[Trash Info]\nPath=/n\n").unwrap();

    assert_eq!(list(&home), vec![]);
}

#[test]
fn the_first_path_and_date_win() {
    let folder = TestFolder::new("read-first");
    let home = trash(&folder, "Trash", None);
    put(
        &home,
        "a",
        b"[Trash Info]\nPath=/first\nPath=/second\nDeletionDate=x\nDeletionDate=2024-03-01T10:20:30\nDeletionDate=2025-03-01T10:20:30\n",
    );

    let listed = list(&home);

    assert_eq!(listed[0].original, PathBuf::from("/first"));
    let naive = NaiveDateTime::parse_from_str("2024-03-01T10:20:30", DATE_FORMAT).unwrap();
    let local = Local.from_local_datetime(&naive).earliest().unwrap();
    assert_eq!(listed[0].deleted, Some(SystemTime::from(local)));
}

#[test]
fn an_info_file_too_big_is_left_out() {
    let folder = TestFolder::new("read-big");
    let home = trash(&folder, "Trash", None);
    let mut text = b"[Trash Info]\nPath=/a\n".to_vec();
    text.resize(usize::try_from(MAX_INFO_BYTES).unwrap() + 1, b'\n');
    put(&home, "a", &text);

    assert_eq!(list(&home), vec![]);
}

#[test]
fn an_info_file_of_the_most_bytes_is_read() {
    let folder = TestFolder::new("read-most");
    let home = trash(&folder, "Trash", None);
    let mut text = b"[Trash Info]\nPath=/a\n".to_vec();
    // A long path's worth, and the most there may be.
    text.resize(4096, b'\n');
    put(&home, "a", &text);
    text.resize(usize::try_from(MAX_INFO_BYTES).unwrap(), b'\n');
    put(&home, "b", &text);

    assert_eq!(list(&home).len(), 2);
}

#[test]
fn a_trash_found_twice_counts_once() {
    let at = |root: &str| TrashFolder {
        root: root.into(),
        topdir: Some("/mnt".into()),
    };
    let mut found = vec![at("/mnt/.Trash-1")];

    add_new(&mut found, at("/mnt/.Trash-1"));
    add_new(&mut found, at("/usb/.Trash-1"));

    assert_eq!(found, vec![at("/mnt/.Trash-1"), at("/usb/.Trash-1")]);
}

#[test]
fn finds_an_entry_only_in_a_known_trash() {
    let folder = TestFolder::new("read-find");
    let home = trash(&folder, "Trash", None);
    let other = trash(&folder, "Other", None);
    let item = put(&home, "a", b"[Trash Info]\nPath=/a\n");
    let stray = put(&other, "a", b"[Trash Info]\nPath=/a\n");
    let trashes = Trashes {
        home: Some(home.root.clone()),
        drives: true,
    };

    let (found_in, found) = find(&item, &trashes).unwrap();

    assert_eq!(found_in, home);
    assert_eq!(found.original, PathBuf::from("/a"));
    assert!(find(&stray, &trashes).is_none());
    assert!(find(&home.files().join("missing"), &trashes).is_none());
    assert!(find(&home.files(), &trashes).is_none());
    assert!(find(&home.info().join("a.trashinfo"), &trashes).is_none());
}

#[test]
fn an_info_file_that_is_a_link_or_no_file_is_left_out() {
    let folder = TestFolder::new("read-odd-info");
    let home = trash(&folder, "Trash", None);
    let real = folder.file("real", "[Trash Info]\nPath=/a\n");
    fs::write(home.files().join("link"), "x").unwrap();
    std::os::unix::fs::symlink(&real, info_of(&home, OsStr::new("link"))).unwrap();
    fs::write(home.files().join("pipe"), "x").unwrap();
    rustix::fs::mknodat(
        rustix::fs::CWD,
        info_of(&home, OsStr::new("pipe")),
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        0,
    )
    .unwrap();

    assert_eq!(list(&home), vec![], "never blocks on the pipe either");
}

#[test]
fn a_home_trash_through_a_link_inside_it_is_not_one() {
    let folder = TestFolder::new("read-linked-files");
    let elsewhere = folder.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    let root = folder.path().join("Trash");
    fs::create_dir_all(root.join("info")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, root.join("files")).unwrap();

    assert_eq!(trashes(Some(&root), false), vec![]);
}

#[test]
fn a_drive_trash_whole_path_must_stay_on_the_drive() {
    let folder = TestFolder::new("read-drive-whole");
    let top = folder.path().join("drive");
    let drive = trash(&folder, "drive/.Trash-1000", Some(&top));
    put(
        &drive,
        "in",
        format!("[Trash Info]\nPath={}/a\n", top.display()).as_bytes(),
    );
    put(&drive, "out", b"[Trash Info]\nPath=/home/me/a\n");
    put(
        &drive,
        "up",
        format!("[Trash Info]\nPath={}/../a\n", top.display()).as_bytes(),
    );

    let listed = list(&drive);

    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].original, top.join("a"));
}

#[test]
fn the_home_trash_is_found_when_it_is_there() {
    let folder = TestFolder::new("read-trashes");
    let home = trash(&folder, "Trash", None);

    assert_eq!(trashes(Some(&home.root), false), vec![home.clone()]);
    assert_eq!(trashes(Some(&folder.path().join("none")), false), vec![]);
    assert_eq!(trashes(None, false), vec![]);
    let with_drives = trashes(Some(&home.root), true);
    assert_eq!(with_drives.first(), Some(&home));
    assert!(with_drives[1..].iter().all(|other| other.topdir.is_some()));
}

#[test]
fn the_stored_name_is_the_info_files_name_less_its_suffix() {
    assert_eq!(stored_of(OsStr::new("a.b.trashinfo")), Some("a.b".into()));
    assert_eq!(stored_of(OsStr::new(".trashinfo")), None);
    assert_eq!(stored_of(OsStr::new("a.trash")), None);
}
