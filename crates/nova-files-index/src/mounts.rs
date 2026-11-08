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
//! mounts without mounting anything.

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
        let asked = statx(CWD, folder, AtFlags::SYMLINK_NOFOLLOW, StatxFlags::empty());
        match asked {
            Ok(answer)
                if answer
                    .stx_attributes_mask
                    .contains(StatxAttributes::MOUNT_ROOT) =>
            {
                answer.stx_attributes.contains(StatxAttributes::MOUNT_ROOT)
            }
            _ => self
                .listed
                .get_or_init(|| {
                    fs::read(MOUNTINFO).map_or_else(|_| HashSet::new(), |text| mount_points(&text))
                })
                .contains(folder),
        }
    }
}

/// The mount points a mountinfo file lists.
fn mount_points(mountinfo: &[u8]) -> HashSet<PathBuf> {
    mountinfo
        .split(|&byte| byte == b'\n')
        .filter_map(|line| line.split(|&byte| byte == b' ').nth(MOUNT_POINT_FIELD))
        .map(|field| PathBuf::from(OsString::from_vec(unescape(field))))
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
    fn a_backslash_without_three_octal_digits_stays() {
        assert_eq!(unescape(b"a\\9zz"), b"a\\9zz");
        assert_eq!(unescape(b"end\\04"), b"end\\04");
        assert_eq!(unescape(b"\\011x"), b"\tx");
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
