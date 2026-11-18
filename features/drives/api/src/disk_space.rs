//! How full the drives holding some paths are, as the system reports it.
//!
//! A client asks with [`Command::DiskSpace`], naming any paths (a drive's
//! mount point, the root), and hears [`Event::DiskSpace`] with one answer
//! per path, in the order asked. The numbers are as of the ask; a client
//! that shows them asks again when it shows them again.
//!
//! [`Command::DiskSpace`]: crate::Command::DiskSpace
//! [`Event::DiskSpace`]: crate::Event::DiskSpace

use std::path::PathBuf;

/// The most paths one [`crate::Command::DiskSpace`] may name: more drives
/// than a computer has. The core leaves out any past it.
pub const MAX_DISK_SPACE_PATHS: usize = 64;

/// A whole drive, in percent.
const WHOLE: u8 = 100;

/// How big the drive holding a path is, and how much of it is free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiskSpace {
    /// Its size in bytes.
    pub total: u64,
    /// The bytes this user can still write. Less than what is unused when
    /// the drive keeps some back for the system.
    pub free: u64,
}

impl DiskSpace {
    /// How much is used, in whole percent from 0 to 100, counting space
    /// kept back for the system as used. A drive with no size is empty.
    pub fn percent_used(self) -> u8 {
        if self.total == 0 {
            return 0;
        }
        let used = u128::from(self.total.saturating_sub(self.free));
        let percent = used * u128::from(WHOLE) / u128::from(self.total);
        u8::try_from(percent).unwrap_or(WHOLE).min(WHOLE)
    }
}

/// One path's answer: its drive's space, or `None` when the system
/// couldn't say (the path is missing, or the drive stopped answering).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathSpace {
    pub path: PathBuf,
    pub space: Option<DiskSpace>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_used_rounds_down_and_stays_within_0_to_100() {
        let space = |total, free| DiskSpace { total, free };
        assert_eq!(space(0, 0).percent_used(), 0);
        assert_eq!(space(100, 100).percent_used(), 0);
        assert_eq!(space(100, 0).percent_used(), 100);
        assert_eq!(space(1000, 381).percent_used(), 61);
        assert_eq!(space(3, 1).percent_used(), 66);
        assert_eq!(space(10, 20).percent_used(), 0);
        assert_eq!(space(u64::MAX, 0).percent_used(), 100);
        assert_eq!(space(u64::MAX, u64::MAX / 2).percent_used(), 50);
    }
}
