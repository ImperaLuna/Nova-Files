//! A change to who owns files and folders and who may do what with them,
//! for [`Operation::ChangeAccess`](crate::Operation::ChangeAccess).

/// The permission bits: read, write and run for owner, group and others,
/// plus set-user, set-group and sticky.
pub const MODE_BITS: u32 = 0o7777;

/// Permission bits to turn on and off, from [`MODE_BITS`]. Bits in neither stay as they are; a bit in both ends up on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ModeChange {
    pub set: u32,
    pub clear: u32,
}

impl ModeChange {
    /// What `mode`'s permission bits become.
    #[must_use]
    pub const fn applied_to(self, mode: u32) -> u32 {
        (mode & !self.clear | self.set) & MODE_BITS
    }
}

/// How far a change reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Reach {
    /// Only the paths chosen.
    #[default]
    Chosen,
    /// The paths chosen and everything in the folders among them, at
    /// every depth, except what is behind a link or on another drive
    /// mounted inside.
    Inside,
}

/// What to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct AccessChange {
    /// The new owner's user number; `None` keeps each one's own.
    pub owner: Option<u32>,
    /// The new group's number; `None` keeps each one's own.
    pub group: Option<u32>,
    /// What changes on files (and devices, pipes and sockets).
    pub files: ModeChange,
    /// What changes on folders.
    pub folders: ModeChange,
    pub reach: Reach,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_sets_and_clears_only_permission_bits() {
        let change = ModeChange {
            set: 0o640,
            clear: 0o027,
        };

        // Bits already set stay set.
        assert_eq!(change.applied_to(0o644), 0o640);
        assert_eq!(change.applied_to(0o100_000 | 0o7777), 0o7750);
    }
}
