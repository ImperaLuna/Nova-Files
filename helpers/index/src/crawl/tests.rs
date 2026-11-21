use std::collections::HashSet;

use nova_files_test_folder::TestFolder;

use super::*;

struct Mounts(HashSet<PathBuf>);

impl MountCheck for Mounts {
    fn is_mount_root(&self, folder: &Path) -> bool {
        self.0.contains(folder)
    }
}

struct Home(TestFolder);

impl Home {
    fn new(name: &str) -> Self {
        let folder = TestFolder::new(&format!("crawl-{name}"));
        fs::create_dir_all(folder.join("drive")).unwrap();
        fs::write(folder.join("drive/inside.txt"), b"").unwrap();
        Self(folder)
    }
}

fn indexed(root: &Path, mounts: Mounts) -> crate::Index {
    let built = crawl_with(root, mounts, || false).unwrap();
    let folder = TestFolder::new("crawl-index");
    let path = folder.join("index");
    built.save(&path).unwrap();
    crate::Index::open(&path).unwrap()
}

#[test]
fn a_mount_point_is_indexed_but_not_entered() {
    let home = Home::new("mount");
    let mounts = Mounts(std::iter::once(home.0.join("drive")).collect());
    let index = indexed(home.0.path(), mounts);
    assert_eq!(index.rows(), 2);
    let drive = index.entry(RowId(1)).unwrap();
    assert!(drive.flags.contains(Flags::OTHER_DRIVE));
    assert_eq!(drive.end, RowId(2));
}

#[test]
fn a_folder_that_isnt_a_mount_is_entered() {
    let home = Home::new("plain");
    let index = indexed(home.0.path(), Mounts(HashSet::new()));
    assert_eq!(index.rows(), 3);
    assert_eq!(index.entry(RowId(1)).unwrap().flags, Flags::default());
}

#[test]
fn a_folder_with_its_own_device_that_isnt_a_mount_is_entered() {
    let dev = Path::new("/dev");
    let device = fs::metadata(dev).unwrap().dev();
    let Some(other) = fs::read_dir(dev).unwrap().flatten().find(|item| {
        let name = item.file_name();
        item.metadata()
            .is_ok_and(|details| details.is_dir() && details.dev() != device)
            && policy::enters(&Zone::Home, &name).is_some()
            && fs::read_dir(item.path()).is_ok_and(|mut inside| inside.next().is_some())
    }) else {
        return;
    };
    let index = indexed(dev, Mounts(HashSet::new()));
    let row = (0..index.rows())
        .map(RowId)
        .find(|&row| index.path(row).unwrap() == other.path())
        .unwrap();
    let entry = index.entry(row).unwrap();
    assert!(!entry.flags.contains(Flags::OTHER_DRIVE));
    assert!(entry.end.0 > row.0 + 1, "{:?} was entered", other.path());
}

#[test]
fn only_a_missing_entry_counts_as_gone() {
    assert!(matches!(stop(io::ErrorKind::NotFound.into()), Stop::Gone));
    assert!(matches!(
        stop(io::ErrorKind::PermissionDenied.into()),
        Stop::Failed(error) if error.kind() == io::ErrorKind::PermissionDenied
    ));
}
