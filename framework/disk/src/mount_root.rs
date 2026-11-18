use rustix::fs::StatxAttributes;

pub type Device = (u32, u32);

// STATX_ATTR_MOUNT_ROOT needs Linux 5.8; a btrfs subvolume has its own device number without being a mount.
// https://man7.org/linux/man-pages/man2/statx.2.html
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
        assert!(!is_mount_root(root, none, other, one));
        assert!(is_mount_root(none, none, other, one));
        assert!(!is_mount_root(none, none, one, one));
    }
}
