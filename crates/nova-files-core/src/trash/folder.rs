//! Which trash an entry goes to: the home trash when the entry is on the
//! home trash's mount, otherwise its own mount's trash at the mount's top
//! folder, `.Trash/<uid>` when the admin made a shared `.Trash` (a real
//! folder with the sticky bit), else `.Trash-<uid>`. Either is made when
//! missing and used only while it is a real folder the user owns.
//!
//! A trash takes entries by rename only, so it must be where a rename
//! reaches: on the same mount (the kernel's mount id, `statx`'s
//! `STATX_MNT_ID`, which also tells a bind mount from what it shows) and
//! on the same device number, as btrfs refuses a rename from one
//! subvolume to another on one mount. A subvolume's top folder counts as
//! a mount's top, as other file managers take it. Kernels before 5.8
//! don't give the mount id; there the device number alone tells.

use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};
use std::path::{Path, PathBuf};

use rustix::fs::{AtFlags, CWD, StatxFlags, statx};
use rustix::process::getuid;

use super::{FILES, INFO};
use crate::private_file::FOLDER_MODE;

/// The sticky bit, which a shared `.Trash` must have.
const STICKY: u32 = 0o1000;
/// The shared trash folder an admin may make at a mount's top.
pub(super) const SHARED: &str = ".Trash";
/// The start of a user's own trash folder at a mount's top.
pub(super) const OWN_PREFIX: &str = ".Trash-";

/// One trash: its folder, and the top folder of its mount for a trash
/// that isn't the home trash, which its info files' paths are relative
/// to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashFolder {
    pub root: PathBuf,
    pub topdir: Option<PathBuf>,
}

impl TrashFolder {
    /// Where its trashed entries sit.
    pub fn files(&self) -> PathBuf {
        self.root.join(FILES)
    }

    /// Where its info files sit.
    pub fn info(&self) -> PathBuf {
        self.root.join(INFO)
    }

    /// `original`, a path without links, as this trash's info files
    /// write it: whole for the home trash, from the mount's top
    /// otherwise.
    pub fn path_field<'a>(&self, original: &'a Path) -> &'a Path {
        self.topdir
            .as_ref()
            .and_then(|top| original.strip_prefix(top).ok())
            .unwrap_or(original)
    }
}

/// The trash for entries in `folder`, a folder whose path holds no links,
/// made ready to take one: the home trash at `home_trash` when a rename
/// reaches it, else the one at the top of `folder`'s mount.
///
/// # Errors
/// When `folder` can't be looked at, or no trash on its mount can be
/// used or made.
pub fn folder_for(folder: &Path, home_trash: Option<&Path>) -> io::Result<TrashFolder> {
    let mount = mount_of(folder, AtFlags::SYMLINK_NOFOLLOW)?;
    let home_reached = home_trash.filter(|home| match home_mount(home) {
        Ok(home) => home == mount,
        Err(error) => {
            log::info!("can't find the home trash {}: {error}", home.display());
            false
        }
    });
    if let Some(home) = home_reached {
        // Made with its parents when missing; a link the user made to
        // where they keep it is followed.
        DirBuilder::new()
            .recursive(true)
            .mode(FOLDER_MODE)
            .create(home)?;
        let root = home.canonicalize()?;
        own_folder(&root)?;
        return Ok(TrashFolder { root, topdir: None });
    }
    let top = topdir(folder, mount)?;
    let root = mount_trash(&top)?;
    Ok(TrashFolder {
        root,
        topdir: Some(top),
    })
}

/// The mount of the home trash at `home`, or of the nearest folder above
/// it while it doesn't exist yet. Links are followed: the home trash may
/// be a link to where the user keeps it.
fn home_mount(home: &Path) -> io::Result<Mount> {
    for folder in home.ancestors() {
        match mount_of(folder, AtFlags::empty()) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            found => return found,
        }
    }
    Err(io::ErrorKind::NotFound.into())
}

/// Where an entry is, as far as a rename can reach: its mount and its
/// device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mount {
    /// `None` on a kernel that doesn't say.
    id: Option<u64>,
    device: (u32, u32),
}

/// The mount and device `path` is on.
fn mount_of(path: &Path, flags: AtFlags) -> io::Result<Mount> {
    let answer = statx(CWD, path, flags, StatxFlags::MNT_ID)?;
    let id = (answer.stx_mask & StatxFlags::MNT_ID.bits() != 0).then_some(answer.stx_mnt_id);
    Ok(Mount {
        id,
        device: (answer.stx_dev_major, answer.stx_dev_minor),
    })
}

/// The trash `root` is, when it is a mount's: `.Trash-<uid>`, or
/// `<uid>` in a fit shared `.Trash`, at the top of its mount, as
/// [`folder_for`] would pick it, and the user's own. Nothing is made.
pub fn mount_trash_at(root: &Path) -> Option<TrashFolder> {
    let top = trash_top(root)?;
    let mount = mount_of(top, AtFlags::SYMLINK_NOFOLLOW).ok()?;
    let at_top = topdir(top, mount).ok()? == top;
    (at_top && is_own(root)).then(|| TrashFolder {
        root: root.to_path_buf(),
        topdir: Some(top.to_path_buf()),
    })
}

/// Where the mount top would be were `root` a mount's trash, by its
/// name: above `.Trash-<uid>`, or above a fit shared `.Trash` that holds
/// `<uid>`.
fn trash_top(root: &Path) -> Option<&Path> {
    let uid = getuid().as_raw().to_string();
    let name = root.file_name()?;
    let above = root.parent()?;
    if name.to_str() == Some(&format!("{OWN_PREFIX}{uid}")) {
        Some(above)
    } else if name.to_str() == Some(&uid) && above.file_name()? == SHARED && shared_fit(above) {
        above.parent()
    } else {
        None
    }
}

/// Whether `root`, its `files` and its `info` are each a real folder the
/// user owns, never reached through a link.
pub fn is_own(root: &Path) -> bool {
    [root.to_path_buf(), root.join(FILES), root.join(INFO)]
        .iter()
        .all(|folder| fs::symlink_metadata(folder).is_ok_and(|there| owned_folder(&there).is_ok()))
}

/// Whether `shared`, a mount's `.Trash`, is one the admin made for every
/// user: a real folder with the sticky bit.
fn shared_fit(shared: &Path) -> bool {
    shared
        .symlink_metadata()
        .is_ok_and(|there| there.is_dir() && there.mode() & STICKY != 0)
}

/// The top folder of `mount`, which `folder` is on.
fn topdir(folder: &Path, mount: Mount) -> io::Result<PathBuf> {
    let mut top = folder;
    while let Some(above) = top.parent() {
        if mount_of(above, AtFlags::SYMLINK_NOFOLLOW)? != mount {
            break;
        }
        top = above;
    }
    Ok(top.to_path_buf())
}

/// The user's trash at the mount top `top`: in the shared `.Trash` when
/// it is fit, else `.Trash-<uid>`.
fn mount_trash(top: &Path) -> io::Result<PathBuf> {
    let uid = getuid().as_raw();
    let shared = top.join(SHARED);
    if shared_fit(&shared) {
        let own = shared.join(uid.to_string());
        match own_folder(&own) {
            Ok(()) => return Ok(own),
            Err(error) => log::warn!("can't use the trash {}: {error}", own.display()),
        }
    }
    let own = top.join(format!("{OWN_PREFIX}{uid}"));
    own_folder(&own)?;
    Ok(own)
}

/// Makes `root`, with `files` and `info` in it, each a folder of the
/// user's alone, unless there; each must be a real folder the user owns.
fn own_folder(root: &Path) -> io::Result<()> {
    for folder in [root.to_path_buf(), root.join(FILES), root.join(INFO)] {
        match DirBuilder::new().mode(FOLDER_MODE).create(&folder) {
            Err(error) if error.kind() != io::ErrorKind::AlreadyExists => return Err(error),
            _ => owned_folder(&fs::symlink_metadata(&folder)?)?,
        }
    }
    Ok(())
}

/// Checks that `there` is a real folder the user owns.
fn owned_folder(there: &fs::Metadata) -> io::Result<()> {
    if there.is_dir() && there.uid() == getuid().as_raw() {
        Ok(())
    } else {
        Err(io::ErrorKind::PermissionDenied.into())
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    use super::*;
    use crate::ops::TestFolder;

    #[test]
    fn the_top_of_a_mount_is_where_the_mount_changes() {
        let folder = TestFolder::new("trash-topdir");
        let mount = mount_of(folder.path(), AtFlags::SYMLINK_NOFOLLOW).unwrap();

        let top = topdir(folder.path(), mount).unwrap();

        assert!(folder.path().starts_with(&top), "{}", top.display());
        assert_eq!(mount_of(&top, AtFlags::SYMLINK_NOFOLLOW).unwrap(), mount);
        if let Some(above) = top.parent() {
            assert_ne!(mount_of(above, AtFlags::SYMLINK_NOFOLLOW).unwrap(), mount);
        }
    }

    #[test]
    fn the_kernel_names_the_mount() {
        // Linux 5.8 and later give the mount id.
        let mount = mount_of(Path::new("/"), AtFlags::empty()).unwrap();

        assert!(mount.id.is_some());
    }

    #[test]
    fn an_entry_on_the_home_trash_mount_goes_to_the_home_trash() {
        let folder = TestFolder::new("trash-home");
        let home = folder.path().join("share/Trash");

        let trash = folder_for(folder.path(), Some(&home)).unwrap();

        let root = home.canonicalize().unwrap();
        assert_eq!(trash, TrashFolder { root, topdir: None });
        assert!(home.join(FILES).is_dir() && home.join(INFO).is_dir());
        let mode = fs::metadata(&home).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, FOLDER_MODE);
    }

    #[test]
    fn a_home_trash_that_is_a_link_to_a_file_is_refused() {
        let folder = TestFolder::new("trash-link");
        let target = folder.file("not-a-folder", "x");
        let home = folder.path().join("Trash");
        symlink(&target, &home).unwrap();

        assert!(folder_for(folder.path(), Some(&home)).is_err());
    }

    #[test]
    fn a_files_folder_swapped_for_a_link_is_refused() {
        let folder = TestFolder::new("trash-files-link");
        let home = folder.path().join("Trash");
        fs::create_dir_all(home.join(INFO)).unwrap();
        fs::create_dir(folder.path().join("elsewhere")).unwrap();
        symlink(folder.path().join("elsewhere"), home.join(FILES)).unwrap();

        assert!(folder_for(folder.path(), Some(&home)).is_err());
    }

    #[test]
    fn the_mount_top_is_found_above_any_folder_on_it() {
        let folder = TestFolder::new("trash-top");
        let deep = folder.path().join("a/b");
        fs::create_dir_all(&deep).unwrap();
        let mount = mount_of(&deep, AtFlags::SYMLINK_NOFOLLOW).unwrap();

        let from_deep = topdir(&deep, mount).unwrap();
        let from_folder = topdir(folder.path(), mount).unwrap();

        assert_eq!(from_deep, from_folder);
        assert!(deep.starts_with(&from_deep));
    }

    #[test]
    fn a_shared_trash_without_the_sticky_bit_is_passed_over() {
        let folder = TestFolder::new("trash-shared");
        let shared = folder.path().join(SHARED);
        fs::create_dir(&shared).unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o777)).unwrap();

        let root = mount_trash(folder.path()).unwrap();

        let uid = getuid().as_raw();
        assert_eq!(root, folder.path().join(format!("{OWN_PREFIX}{uid}")));
    }

    #[test]
    fn a_shared_trash_with_the_sticky_bit_holds_the_users_own() {
        let folder = TestFolder::new("trash-sticky");
        let shared = folder.path().join(SHARED);
        fs::create_dir(&shared).unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o1777)).unwrap();

        let root = mount_trash(folder.path()).unwrap();

        assert_eq!(root, shared.join(getuid().as_raw().to_string()));
        assert!(root.join(FILES).is_dir());
    }

    #[test]
    fn a_mount_trash_is_known_by_its_name_and_a_fit_shared_trash() {
        let folder = TestFolder::new("trash-top-name");
        let uid = getuid().as_raw().to_string();
        let shared = folder.path().join(SHARED);
        fs::create_dir(&shared).unwrap();
        let own = folder.path().join(format!("{OWN_PREFIX}{uid}"));

        assert_eq!(trash_top(&own), Some(folder.path()));
        assert_eq!(trash_top(&shared.join(&uid)), None, "not sticky");
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o1777)).unwrap();
        assert_eq!(trash_top(&shared.join(&uid)), Some(folder.path()));
        assert_eq!(trash_top(&folder.path().join(&uid)), None, "not in .Trash");
        assert_eq!(
            trash_top(&shared.join("0")),
            (uid == "0").then_some(folder.path())
        );
        assert_eq!(trash_top(&folder.path().join(".Trash-x")), None);
    }

    #[test]
    fn a_drive_trash_writes_paths_from_the_mount_top() {
        let trash = TrashFolder {
            root: "/mnt/usb/.Trash-1000".into(),
            topdir: Some("/mnt/usb".into()),
        };
        let home = TrashFolder {
            root: "/home/me/.local/share/Trash".into(),
            topdir: None,
        };

        assert_eq!(
            trash.path_field(Path::new("/mnt/usb/photos/a.jpg")),
            Path::new("photos/a.jpg")
        );
        assert_eq!(
            home.path_field(Path::new("/home/me/a.txt")),
            Path::new("/home/me/a.txt")
        );
    }
}
