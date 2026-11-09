//! The crawl with made-up mounts: a mount point isn't entered, a folder
//! with its own device number that isn't a mount (a btrfs subvolume) is.

use std::collections::HashSet;

use super::*;

/// Says the folders it holds are mount points, and nothing else is.
struct Mounts(HashSet<PathBuf>);

impl MountCheck for Mounts {
    fn is_mount_root(&self, folder: &Path) -> bool {
        self.0.contains(folder)
    }
}

/// A temp folder with `drive/inside.txt`, removed when dropped.
struct Home(PathBuf);

impl Home {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("nova-files-crawl-{name}-{}", std::process::id()));
        fs::create_dir_all(path.join("drive")).unwrap();
        fs::write(path.join("drive/inside.txt"), b"").unwrap();
        Self(path)
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn indexed(root: &Path, mounts: Mounts) -> crate::Index {
    let built = crawl_with(root, mounts, || false).unwrap();
    let path = std::env::temp_dir().join(format!(
        "nova-files-crawl-index-{}-{}",
        std::process::id(),
        root.as_os_str().len()
    ));
    built.save(&path).unwrap();
    let index = crate::Index::open(&path).unwrap();
    fs::remove_file(&path).ok();
    index
}

#[test]
fn a_mount_point_is_indexed_but_not_entered() {
    let home = Home::new("mount");
    let mounts = Mounts(std::iter::once(home.0.join("drive")).collect());
    let index = indexed(&home.0, mounts);
    assert_eq!(index.rows(), 2);
    let drive = index.entry(RowId(1)).unwrap();
    assert!(drive.flags.contains(Flags::OTHER_DRIVE));
    assert_eq!(drive.end, RowId(2));
}

#[test]
fn a_folder_that_isnt_a_mount_is_entered() {
    let home = Home::new("plain");
    let index = indexed(&home.0, Mounts(HashSet::new()));
    assert_eq!(index.rows(), 3);
    assert_eq!(index.entry(RowId(1)).unwrap().flags, Flags::default());
}

#[test]
fn a_folder_with_its_own_device_that_isnt_a_mount_is_entered() {
    // /dev holds folders with their own device numbers on most systems
    // (pts, shm); with no mounts claimed they stand in for subvolumes.
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
