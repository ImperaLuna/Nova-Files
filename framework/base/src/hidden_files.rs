//! Whether folders list their hidden entries. The listing reads it; the
//! settings feature keeps it with the user's other settings.

/// Whether folders list their hidden entries: those whose name starts
/// with a dot (see [`crate::Entry::is_hidden`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HiddenFiles {
    #[default]
    Left,
    Shown,
}

impl HiddenFiles {
    /// The other choice: what Ctrl+H turns this into.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Left => Self::Shown,
            Self::Shown => Self::Left,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_files_toggle_both_ways() {
        assert_eq!(HiddenFiles::Left.toggled(), HiddenFiles::Shown);
        assert_eq!(HiddenFiles::Shown.toggled(), HiddenFiles::Left);
    }
}
