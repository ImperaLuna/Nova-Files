//! Whether a folder is the root of a mount, which the crawl doesn't enter.
//!
//! A changed device number is not enough: a btrfs subvolume has its own
//! device number without being a mount, and its files belong in the index.
//! [`SystemMounts`] asks the kernel instead, through `statx`'s
//! `STATX_ATTR_MOUNT_ROOT` (Linux 5.8+). On an older kernel, or when that
//! call fails, it looks the folder up among the mount points in
//! `/proc/self/mountinfo`, read once, on first need.
//!
//! The crawl takes any [`MountCheck`], so tests can say which folders are
//! mounts without mounting anything. [`local_mount_points`] lists the
//! mount points of local drives, for the core's trash listing: never a
//! network or FUSE mount, where a look can hang, an automount, which a
//! look would mount, or the kernel's own filesystems.

use std::cell::OnceCell;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};

use rustix::fs::{AtFlags, CWD, StatxAttributes, StatxFlags, statx};

const MOUNTINFO: &str = "/proc/self/mountinfo";
/// The mount point is the fifth field of a mountinfo line.
const MOUNT_POINT_FIELD: usize = 4;
/// Mountinfo writes a space, tab, newline or backslash in a path as a
/// backslash and three octal digits.
const ESCAPE: u8 = b'\\';
const OCTAL_DIGITS: usize = 3;
const OCTAL: u32 = 8;
/// Ends a mountinfo line's optional fields; the filesystem type follows.
const SEPARATOR: &[u8] = b"-";
/// FUSE filesystems' types (`fuse.sshfs`, `fuse.rclone`): often over the
/// network. A local drive through FUSE is `fuseblk`.
const FUSE_PREFIX: &[u8] = b"fuse.";
/// Filesystem types that aren't local drives.
const NOT_LOCAL: &[&[u8]] = &[
    // Over the network.
    b"nfs",
    b"nfs4",
    b"cifs",
    b"smb3",
    b"smbfs",
    b"9p",
    b"afs",
    b"ceph",
    b"glusterfs",
    b"sshfs",
    b"davfs",
    b"fuse",
    // Mounted when looked at.
    b"autofs",
    // The kernel's own.
    b"proc",
    b"sysfs",
    b"devpts",
    b"devtmpfs",
    b"cgroup",
    b"cgroup2",
    b"mqueue",
    b"debugfs",
    b"tracefs",
    b"securityfs",
    b"pstore",
    b"bpf",
    b"configfs",
    b"fusectl",
    b"hugetlbfs",
    b"binfmt_misc",
    b"efivarfs",
    b"rpc_pipefs",
    b"nsfs",
];

/// Answers whether a folder is where a filesystem is mounted.
pub trait MountCheck {
    fn is_mount_root(&self, folder: &Path) -> bool;
}

/// The running system's mounts.
#[derive(Debug, Default)]
pub struct SystemMounts {
    /// From mountinfo, read only if `statx` can't answer.
    listed: OnceCell<HashSet<PathBuf>>,
}

impl MountCheck for SystemMounts {
    fn is_mount_root(&self, folder: &Path) -> bool {
        statx(CWD, folder, AtFlags::SYMLINK_NOFOLLOW, StatxFlags::empty())
            .ok()
            .and_then(|answer| mount_root(answer.stx_attributes_mask, answer.stx_attributes))
            .unwrap_or_else(|| {
                self.listed
                    .get_or_init(system_mount_points)
                    .contains(folder)
            })
    }
}

/// What `statx` says about being a mount root: `None` when the kernel
/// doesn't report it (`mask` lacks it).
fn mount_root(mask: StatxAttributes, attributes: StatxAttributes) -> Option<bool> {
    mask.contains(StatxAttributes::MOUNT_ROOT)
        .then(|| attributes.contains(StatxAttributes::MOUNT_ROOT))
}

/// The mount points the running system has, from mountinfo; none when it
/// can't be read.
fn system_mount_points() -> HashSet<PathBuf> {
    fs::read(MOUNTINFO).map_or_else(|_| HashSet::new(), |text| mount_points(&text))
}

/// The mount points of the running system's local drives, from
/// mountinfo; none when it can't be read.
pub fn local_mount_points() -> HashSet<PathBuf> {
    fs::read(MOUNTINFO).map_or_else(|_| HashSet::new(), |text| local_points(&text))
}

/// The mount points a mountinfo file lists.
fn mount_points(mountinfo: &[u8]) -> HashSet<PathBuf> {
    points_where(mountinfo, |_| true)
}

/// The mount points of local drives a mountinfo file lists.
fn local_points(mountinfo: &[u8]) -> HashSet<PathBuf> {
    points_where(mountinfo, |kind| {
        !NOT_LOCAL.contains(&kind) && !kind.starts_with(FUSE_PREFIX)
    })
}

/// The mount points a mountinfo file lists whose filesystem type `keep`
/// takes.
fn points_where(mountinfo: &[u8], keep: impl Fn(&[u8]) -> bool) -> HashSet<PathBuf> {
    mountinfo
        .split(|&byte| byte == b'\n')
        .filter_map(|line| {
            let fields: Vec<_> = line.split(|&byte| byte == b' ').collect();
            let separator = fields.iter().position(|field| *field == SEPARATOR)?;
            let kind = fields.get(separator + 1)?;
            let point = fields.get(MOUNT_POINT_FIELD).filter(|_| keep(kind))?;
            Some(PathBuf::from(OsString::from_vec(unescape(point))))
        })
        .collect()
}

/// Turns mountinfo's `\040`-style escapes back into bytes.
fn unescape(field: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(field.len());
    let mut rest = field;
    while let Some((&byte, after)) = rest.split_first() {
        let code = after
            .get(..OCTAL_DIGITS)
            .filter(|_| byte == ESCAPE)
            .and_then(|digits| std::str::from_utf8(digits).ok())
            .and_then(|digits| u8::from_str_radix(digits, OCTAL).ok());
        if let Some(code) = code {
            out.push(code);
            rest = after.get(OCTAL_DIGITS..).unwrap_or_default();
        } else {
            out.push(byte);
            rest = after;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_points_come_from_the_fifth_field_unescaped() {
        let mountinfo = b"22 1 0:21 / /proc rw,nosuid - proc proc rw\n\
            36 25 0:33 / /home/me/My\\040NAS rw - cifs //nas/share rw\n\
            37 25 0:34 / /home/me/back\\134slash rw - tmpfs tmpfs rw\n";
        let points = mount_points(mountinfo);
        let expected: HashSet<PathBuf> = ["/proc", "/home/me/My NAS", "/home/me/back\\slash"]
            .iter()
            .map(PathBuf::from)
            .collect();
        assert_eq!(points, expected);
    }

    #[test]
    fn local_drives_leave_out_network_fuse_automount_and_kernel_mounts() {
        let mountinfo = b"22 1 0:21 / /proc rw,nosuid - proc proc rw\n\
            36 25 0:33 / /home/me/NAS rw - cifs //nas/share rw\n\
            37 25 0:34 / /home/me/ssh rw - fuse.sshfs me@host: rw\n\
            38 25 0:35 / /mnt/auto rw - autofs systemd-1 rw\n\
            39 25 8:1 / /run/media/me/USB rw shared:5 - fuseblk /dev/sdb1 rw\n\
            40 1 8:2 / / rw - ext4 /dev/sda2 rw\n\
            bad line\n";
        let points = local_points(mountinfo);
        let expected: HashSet<PathBuf> = ["/run/media/me/USB", "/"]
            .iter()
            .map(PathBuf::from)
            .collect();
        assert_eq!(points, expected);
        assert_eq!(mount_points(mountinfo).len(), 6);
    }

    #[test]
    fn a_backslash_without_three_octal_digits_stays() {
        assert_eq!(unescape(b"a\\9zz"), b"a\\9zz");
        assert_eq!(unescape(b"end\\04"), b"end\\04");
        assert_eq!(unescape(b"\\011x"), b"\tx");
    }

    #[test]
    fn statx_answers_only_when_it_reports_mount_roots() {
        let root = StatxAttributes::MOUNT_ROOT;
        let none = StatxAttributes::empty();
        assert_eq!(mount_root(root, root), Some(true));
        assert_eq!(mount_root(root, none), Some(false));
        assert_eq!(mount_root(none, root), None);
        assert_eq!(
            mount_root(StatxAttributes::IMMUTABLE, StatxAttributes::IMMUTABLE),
            None
        );
    }

    #[test]
    fn the_system_knows_its_own_mounts() {
        let mounts = SystemMounts::default();
        assert!(mounts.is_mount_root(Path::new("/")));
        assert!(!mounts.is_mount_root(&std::env::temp_dir().join("nova-files-surely-not-a-mount")));
        // /proc is a mount nearly everywhere; skip where it isn't.
        let listed = fs::read(MOUNTINFO)
            .map(|text| mount_points(&text))
            .unwrap_or_default();
        if listed.contains(Path::new("/proc")) {
            assert!(mounts.is_mount_root(Path::new("/proc")));
            assert!(!mounts.is_mount_root(Path::new("/proc/self")));
        }
    }

    #[test]
    fn the_mountinfo_fallback_answers_from_the_list() {
        let mounts = SystemMounts::default();
        mounts
            .listed
            .set(std::iter::once(PathBuf::from("/nowhere/mounted")).collect())
            .unwrap();
        // statx fails on a path that doesn't exist, so the list answers.
        assert!(mounts.is_mount_root(Path::new("/nowhere/mounted")));
        assert!(!mounts.is_mount_root(Path::new("/nowhere/else")));
    }
}
