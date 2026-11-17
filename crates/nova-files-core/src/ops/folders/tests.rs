use std::fs;
use std::os::unix::fs::symlink;

use super::*;
use crate::ops::control::Control;
use crate::ops::plan::plan_delete;
use crate::ops::test_folder::TestFolder;

/// The folders of a plan of `paths`.
fn folders_of(paths: &[PathBuf]) -> Folders {
    let plan = plan_delete(paths, &Control::default(), &|_| true).unwrap();
    Folders::of(&plan)
}

/// Which entry `source` names now.
fn entry(source: &Source) -> Entry {
    Entry::of(&source.at().look().unwrap())
}

fn entry_at(path: &Path) -> Entry {
    Entry::of(&path.symlink_metadata().unwrap())
}

#[test]
fn reaches_an_item_through_the_folder_planned() {
    let folder = TestFolder::new("folders-reach");
    let tree = folder.path().join("tree");
    fs::create_dir_all(tree.join("sub")).unwrap();
    let file = folder.file("tree/sub/f", "f");
    let mut folders = folders_of(std::slice::from_ref(&tree));

    let source = folders.source(&file).unwrap();

    assert_eq!(source.at().name, Path::new("f"));
    assert_eq!(source.at().shown, file);
    assert_eq!(source.at().folder_path(), Path::new("."));
    assert_eq!(entry(&source), entry_at(&file));
    assert!(source.at().is_in(&tree.join("sub")));
    assert!(!source.at().is_in(&tree));
}

#[test]
fn a_folder_swapped_for_a_link_is_refused() {
    let folder = TestFolder::new("folders-link");
    let tree = folder.path().join("tree");
    fs::create_dir_all(tree.join("sub")).unwrap();
    folder.file("tree/sub/f", "planned");
    let outside = folder.path().join("outside");
    fs::create_dir(&outside).unwrap();
    folder.file("outside/f", "outside");
    let mut folders = folders_of(std::slice::from_ref(&tree));
    fs::remove_dir_all(tree.join("sub")).unwrap();
    symlink(&outside, tree.join("sub")).unwrap();

    let refused = folders.source(&tree.join("sub/f")).err();

    assert_eq!(refused, Some(Unreached::Changed));
    assert_eq!(
        folders.planned_folder(&tree.join("sub")).err(),
        Some(Unreached::Changed)
    );
}

#[test]
fn a_chosen_folder_swapped_for_a_look_alike_is_refused() {
    let folder = TestFolder::new("folders-look-alike");
    let tree = folder.path().join("tree");
    fs::create_dir(&tree).unwrap();
    folder.file("tree/f", "planned");
    let mut folders = folders_of(std::slice::from_ref(&tree));
    // Another program puts a folder of its own under the chosen name.
    fs::rename(&tree, folder.path().join("moved")).unwrap();
    fs::create_dir(&tree).unwrap();
    folder.file("tree/f", "look-alike");

    assert_eq!(
        folders.source(&tree.join("f")).err(),
        Some(Unreached::Changed)
    );
    assert_eq!(
        folders.planned_folder(&tree).err(),
        Some(Unreached::Changed)
    );
}

#[test]
fn a_folder_reached_once_stays_the_one_planned() {
    let folder = TestFolder::new("folders-held");
    let tree = folder.path().join("tree");
    fs::create_dir(&tree).unwrap();
    let file = folder.file("tree/f", "planned");
    let mut folders = folders_of(std::slice::from_ref(&tree));
    let planned = entry_at(&file);
    assert!(folders.planned_folder(&tree).is_ok());
    folders.open(&tree).unwrap();
    // Moved away while held open: the job still works in the folder planned.
    fs::rename(&tree, folder.path().join("moved")).unwrap();
    fs::create_dir(&tree).unwrap();
    folder.file("tree/f", "look-alike");

    assert_eq!(entry(&folders.source(&tree.join("f")).unwrap()), planned);
}

#[test]
fn where_the_user_named_it_a_link_is_followed() {
    let folder = TestFolder::new("folders-named");
    let tree = folder.path().join("tree");
    let real = folder.path().join("real");
    fs::create_dir(&tree).unwrap();
    fs::create_dir(&real).unwrap();
    let file = folder.file("real/f", "f");
    symlink(&real, tree.join("link")).unwrap();
    // The tree, and a file named through a link inside it.
    let named = tree.join("link/f");
    let mut folders = folders_of(&[tree, named.clone()]);

    let source = folders.source(&named).unwrap();

    assert_eq!(entry(&source), entry_at(&file));
    assert!(source.at().is_in(&named.with_file_name("")));
}

#[test]
fn a_path_named_in_full_is_in_its_parent() {
    let folder = TestFolder::new("folders-at-path");
    let file = folder.file("f", "f");

    assert_eq!(At::path(&file).folder_path(), folder.path());
    assert!(At::path(&file).is_in(folder.path()));
    assert!(!At::path(&file).is_in(&file));
    assert!(At::path(&file).look().unwrap().is_file());
    assert_eq!(
        Folders::default().source(Path::new("/")).err(),
        Some(Unreached::Failed(Errno::INVAL))
    );
}

#[test]
fn a_link_or_file_where_a_folder_was_is_a_change() {
    assert_eq!(Unreached::from(Errno::LOOP), Unreached::Changed);
    assert_eq!(Unreached::from(Errno::NOTDIR), Unreached::Changed);
    assert_eq!(
        Unreached::from(Errno::NOENT),
        Unreached::Failed(Errno::NOENT)
    );
    let changed = io::Error::from(Unreached::Changed);
    assert!(changed.to_string().contains("changed"));
    assert_eq!(
        io::Error::from(Unreached::Failed(Errno::NOENT)).kind(),
        io::ErrorKind::NotFound
    );
}
