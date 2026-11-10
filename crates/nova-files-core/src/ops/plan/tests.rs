use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixListener;

use super::*;
use crate::ops::test_folder::TestFolder;
use nova_files_api::Outcome;

#[test]
fn pairs_must_be_whole_paths_and_never_into_themselves() {
    let pair = |source: &str, target: &str| (PathBuf::from(source), PathBuf::from(target));
    let refused = |pairs: &[(PathBuf, PathBuf)]| check_pairs(pairs).err().map(Stop::outcome);
    let failed = |path: &str, reason| {
        Some(Outcome::Failed {
            path: path.into(),
            reason,
        })
    };

    let folder = TestFolder::new("plan-pairs");
    let there = folder.file("a", "a");
    let whole = (there, folder.path().join("b"));
    assert_eq!(refused(&[whole]), None);
    assert_eq!(
        refused(&[pair("/a", "b")]),
        failed("b", Failure::InvalidName)
    );
    assert_eq!(
        refused(&[pair("/a", "/")]),
        failed("/", Failure::InvalidName)
    );
    assert_eq!(
        refused(&[pair("a", "/b")]),
        failed("a", Failure::InvalidName)
    );
    assert_eq!(
        refused(&[pair("/", "/b")]),
        failed("/", Failure::InvalidName)
    );
    assert_eq!(
        refused(&[pair("/a", "/a/b/c")]),
        failed("/a", Failure::IntoItself)
    );
    assert!(refused(&[pair("/gone", "/b")]).is_some());
}

#[test]
fn plans_folders_before_what_they_hold() {
    let folder = TestFolder::new("plan-tree");
    let source = folder.path().join("src");
    fs::create_dir_all(source.join("sub")).unwrap();
    fs::write(source.join("sub/b"), "bb").unwrap();
    fs::write(source.join("a"), "a").unwrap();
    symlink("a", source.join("link")).unwrap();
    let into = folder.path().join("into");

    let plan = plan(std::slice::from_ref(&source), &into, &Control::default()).unwrap();

    assert_eq!(plan.bytes, 3);
    let made: Vec<_> = plan
        .items
        .iter()
        .map(|item| (item.to.clone(), item.kind))
        .collect();
    assert_eq!(made[0], (into.join("src"), Kind::Folder));
    let position = |path: &Path| made.iter().position(|(to, _)| to == path).unwrap();
    assert!(position(&into.join("src/sub")) < position(&into.join("src/sub/b")));
    assert!(made.contains(&(into.join("src/a"), Kind::File { size: 1 })));
    assert!(made.contains(&(into.join("src/link"), Kind::Link)));
    assert_eq!(made.len(), 5);
    assert!(
        plan.items
            .iter()
            .all(|item| item.to.strip_prefix(&into).unwrap()
                == item.from.strip_prefix(folder.path()).unwrap())
    );
}

#[test]
fn a_link_to_its_own_folder_is_planned_once() {
    let folder = TestFolder::new("plan-loop");
    let source = folder.path().join("src");
    fs::create_dir(&source).unwrap();
    symlink(&source, source.join("again")).unwrap();

    let plan = plan(&[source], &folder.path().join("into"), &Control::default()).unwrap();

    assert_eq!(plan.items.len(), 2);
}

#[test]
fn a_folder_cant_go_inside_itself() {
    let folder = TestFolder::new("plan-itself");
    let source = folder.path().join("src");
    fs::create_dir_all(source.join("sub")).unwrap();

    for into in [source.join("sub"), source.clone()] {
        assert_eq!(
            plan(std::slice::from_ref(&source), &into, &Control::default()),
            Err(Stop::because(&source, Failure::IntoItself))
        );
    }
}

#[test]
fn refuses_what_it_cant_plan_and_notes_what_it_cant_copy() {
    let folder = TestFolder::new("plan-refused");
    let socket = folder.path().join("socket");
    let _listener = UnixListener::bind(&socket).unwrap();
    let into = folder.path().join("into");
    let control = Control::default();

    assert_eq!(
        plan(std::slice::from_ref(&socket), &into, &control)
            .unwrap()
            .items[0]
            .kind,
        Kind::Special
    );
    assert_eq!(
        plan(&[PathBuf::from("/")], &into, &control),
        Err(Stop::because(Path::new("/"), Failure::InvalidName))
    );
    assert_eq!(
        plan(&[PathBuf::from("a")], &into, &control),
        Err(Stop::because(Path::new("a"), Failure::InvalidName))
    );
    assert_eq!(
        plan(std::slice::from_ref(&socket), Path::new("into"), &control),
        Err(Stop::because(Path::new("into"), Failure::InvalidName))
    );
    let gone = folder.path().join("gone");
    assert_eq!(
        plan(std::slice::from_ref(&gone), &into, &control),
        Err(Stop::because(&gone, Failure::NotFound))
    );
}

#[test]
fn stops_when_cancelled() {
    let folder = TestFolder::new("plan-cancel");
    let source = folder.file("a", "a");
    let control = Control::default();
    control.cancel();

    assert_eq!(
        plan(&[source], &folder.path().join("into"), &control),
        Err(Stop::Cancelled)
    );
}

#[test]
fn a_delete_within_leaves_out_what_it_doesnt_want_unread() {
    let folder = TestFolder::new("plan-within");
    let tree = folder.path().join("tree");
    fs::create_dir_all(tree.join("new")).unwrap();
    fs::write(tree.join("new/x"), "x").unwrap();
    fs::write(tree.join("a"), "a").unwrap();
    let new = tree.join("new");
    let paths = [tree.clone()];

    let plan = plan_delete(&paths, &Control::default(), &|path| path != new).unwrap();

    let planned: Vec<_> = plan.items.iter().map(|item| item.from.clone()).collect();
    assert_eq!(planned, [tree.clone(), tree.join("a")]);
}
