//! Whether a folder sorted by size measures its folders. The listing reads
//! it; the settings feature keeps it with the user's other settings.

/// Whether a listing sorted by [`crate::SortKey::Size`] measures how much
/// each of its folders holds, and sorts folders and files together by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FolderSizes {
    Left,
    #[default]
    Measured,
}

impl FolderSizes {
    /// The other choice: what the menu's tick turns this into.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Left => Self::Measured,
            Self::Measured => Self::Left,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_sizes_toggle_both_ways() {
        assert_eq!(FolderSizes::Left.toggled(), FolderSizes::Measured);
        assert_eq!(FolderSizes::Measured.toggled(), FolderSizes::Left);
    }
}
