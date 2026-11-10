use std::fs::{self, File, Permissions};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _, symlink};
use std::sync::mpsc;

use rustix::process::{getegid, geteuid};

use super::*;
use crate::ops::test_folder::TestFolder;

/// Owner reads and writes, group reads, others nothing.
const PRIVATE_FILE: u32 = 0o640;
/// Owner enters, group and others nothing, and the sticky bit.
const STICKY_FOLDER: u32 = 0o1700;
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

const ID: PropertiesId = PropertiesId(7);

fn properties(path: &Path) -> Properties {
    match event(ID, path.to_owned()) {
        Event::Properties { id: ID, properties } => Arc::unwrap_or_clone(properties),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn a_file_tells_its_size_times_mode_and_owner() {
    let folder = TestFolder::new("properties-file");
    let path = folder.path().join("notes");
    fs::write(&path, b"hello").unwrap();
    fs::set_permissions(&path, Permissions::from_mode(PRIVATE_FILE)).unwrap();
    let written = fs::metadata(&path).unwrap();

    let read = properties(&path);

    assert_eq!(read.path, path);
    assert_eq!(read.kind, PathKind::File);
    assert_eq!(read.bytes, 5);
    assert_eq!(read.mode, PRIVATE_FILE);
    assert_eq!(read.modified, Some(written.modified().unwrap()));
    assert_eq!(read.accessed, Some(written.accessed().unwrap()));
    assert!(read.changed.is_some());
    assert_eq!(read.owner.id, geteuid().as_raw());
    assert_eq!(read.owner.name, user_name(geteuid().as_raw()));
    assert_eq!(read.group.id, written.gid());
    assert!(read.may_change);
    assert_eq!(read.read_only, Some(false));
}

#[test]
fn a_folder_keeps_its_sticky_bit_and_its_on_disk_size() {
    let folder = TestFolder::new("properties-folder");
    let inside = folder.path().join("inside");
    fs::create_dir(&inside).unwrap();
    fs::set_permissions(&inside, Permissions::from_mode(STICKY_FOLDER)).unwrap();

    let read = properties(&inside);

    assert_eq!(read.kind, PathKind::Folder);
    assert_eq!(read.mode, STICKY_FOLDER);
    assert_eq!(
        read.on_disk,
        fs::metadata(&inside).unwrap().blocks() * BLOCK
    );
}

#[test]
fn a_link_is_itself_with_the_target_as_written() {
    let folder = TestFolder::new("properties-link");
    File::create(folder.path().join("target")).unwrap();
    let link = folder.path().join("link");
    symlink("target", &link).unwrap();
    let broken = folder.path().join("broken");
    symlink("nowhere/at/all", &broken).unwrap();

    let read = properties(&link);

    assert_eq!(
        read.kind,
        PathKind::Link {
            target: "target".into()
        }
    );
    assert_eq!(read.bytes, 6);
    assert!(!read.may_change);
    assert_eq!(
        properties(&broken).kind,
        PathKind::Link {
            target: "nowhere/at/all".into()
        }
    );
}

#[test]
fn a_device_is_special() {
    assert_eq!(properties(Path::new("/dev/null")).kind, PathKind::Special);
}

#[test]
fn a_missing_path_says_so() {
    let folder = TestFolder::new("properties-missing");
    let gone = folder.path().join("gone");

    assert_eq!(
        event(ID, gone.clone()),
        Event::PropertiesUnreadable {
            id: ID,
            path: gone,
            reason: Unreadable::Missing
        }
    );
}

#[test]
fn the_user_s_own_groups_come_named() {
    let folder = TestFolder::new("properties-groups");

    let read = properties(folder.path());

    let mine = getegid().as_raw();
    assert!(
        read.groups.contains(&Account {
            id: mine,
            name: group_name(mine),
        }),
        "{:?}",
        read.groups
    );
    assert_eq!(read.groups.len(), own_groups().len());
}

#[test]
fn the_owner_and_the_superuser_may_change_access_and_no_one_else() {
    let (owner, other) = (1000, 1001);

    assert!(may_change(owner, owner));
    assert!(may_change(ROOT, owner));
    assert!(!may_change(other, owner));
}

#[test]
fn a_time_the_drive_did_not_fill_in_is_none() {
    let folder = TestFolder::new("properties-mask");
    let stat = statx(CWD, folder.path(), AtFlags::empty(), WANTED).unwrap();
    let modified = Some(fs::metadata(folder.path()).unwrap().modified().unwrap());

    assert_eq!(
        time(stat.stx_mask, StatxFlags::MTIME, stat.stx_mtime),
        modified
    );
    assert_eq!(time(0, StatxFlags::MTIME, stat.stx_mtime), None);
}

#[test]
fn times_before_the_epoch_and_past_what_the_system_holds() {
    let half = Duration::from_millis(500);

    assert_eq!(
        since_epoch(2, 500_000_000),
        Some(SystemTime::UNIX_EPOCH + Duration::from_secs(2) + half)
    );
    assert_eq!(
        since_epoch(-1, 500_000_000),
        Some(SystemTime::UNIX_EPOCH - half)
    );
    assert_eq!(since_epoch(i64::MAX, u32::MAX), None);
}

#[test]
fn an_answer_comes_from_a_thread_of_its_own() {
    let folder = TestFolder::new("properties-answer");
    let (sink, events) = mpsc::channel();
    let sink: Arc<dyn EventSink> = Arc::new(sink);

    answer(&sink, ID, folder.path().to_owned());

    match events.recv_timeout(ANSWER_TIMEOUT).unwrap() {
        Event::Properties { id: ID, properties } => {
            assert_eq!(properties.kind, PathKind::Folder);
        }
        other => panic!("unexpected {other:?}"),
    }
}
