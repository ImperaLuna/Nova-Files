//! Where another drive is mounted, for walks that stay on one drive: the
//! size scan and changing access.

use rustix::fs::StatxAttributes;

/// A drive, as the major and minor numbers `statx` gives.
pub type Device = (u32, u32);

/// Whether a folder on drive `own`, inside a folder on drive `parent`, is
/// where another filesystem is mounted. The kernel says so in the folder's
/// `statx` attributes (Linux 5.8+); a changed drive alone would also stop
/// at a btrfs subvolume, which is no mount, so it only answers on older
/// kernels, whose `mask` lacks the attribute.
pub fn is_mount_root(
    mask: StatxAttributes,
    attributes: StatxAttributes,
    own: Device,
    parent: Device,
) -> bool {
    if mask.contains(StatxAttributes::MOUNT_ROOT) {
        return attributes.contains(StatxAttributes::MOUNT_ROOT);
    }
    own != parent
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kernel_says_where_a_drive_is_mounted_and_older_kernels_go_by_the_drive() {
        let root = StatxAttributes::MOUNT_ROOT;
        let none = StatxAttributes::empty();
        let (one, other) = ((8, 1), (8, 2));

        assert!(is_mount_root(root, root, one, one));
        // A btrfs subvolume: another drive number, but no mount.
        assert!(!is_mount_root(root, none, other, one));
        assert!(is_mount_root(none, none, other, one));
        assert!(!is_mount_root(none, none, one, one));
    }
}
