//! How a folder is shown: as rows with columns, as a grid of icons at one
//! of five sizes, or as a gallery of one big preview. Each folder keeps
//! the view it was last given; the core saves them between runs.
//!
//! A client asks with [`Command::Views`] and gets [`Event::Views`] then and
//! after every change; [`Command::SetView`] gives one folder its view.

#![forbid(unsafe_code)]

use std::path::PathBuf;

/// The most folders whose views are kept; the oldest set goes first.
pub const MAX_FOLDER_VIEWS: usize = 1000;

/// The way a folder's entries are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ViewMode {
    /// One row per entry, with columns.
    #[default]
    Details,
    /// A grid of icons and thumbnails.
    Icons,
    /// One entry large, with a strip of the others under it.
    Gallery,
}

/// How big the icons of [`ViewMode::Icons`] are, smallest first. The
/// window turns each into its own size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum IconSize {
    Smallest,
    Small,
    #[default]
    Medium,
    Large,
    Largest,
}

impl IconSize {
    /// Every size, smallest first.
    pub const ALL: [Self; 5] = [
        Self::Smallest,
        Self::Small,
        Self::Medium,
        Self::Large,
        Self::Largest,
    ];

    /// The next size up; the largest stays.
    #[must_use]
    pub fn bigger(self) -> Self {
        self.stepped(1)
    }

    /// The next size down; the smallest stays.
    #[must_use]
    pub fn smaller(self) -> Self {
        self.stepped(-1)
    }

    /// Where it stands in [`Self::ALL`].
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// `by` sizes up, or down when negative, kept to the sizes there are.
    #[must_use]
    pub fn stepped(self, by: isize) -> Self {
        let last = Self::ALL.len() - 1;
        Self::ALL[self.index().saturating_add_signed(by).min(last)]
    }
}

/// How one folder is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FolderView {
    pub mode: ViewMode,
    /// The icons' size, kept while another mode shows.
    pub icons: IconSize,
}

/// A folder and the view it was given.
pub type FolderViews = std::sync::Arc<[(PathBuf, FolderView)]>;

/// A request from a client to the folders feature's core half.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Ask for the folders' views: the answer is [`Event::Views`], sent
    /// again whenever one changes.
    Views,
    /// Show `folder` with `view` from now on, and save it. Every client
    /// that asked for the views, this one included, then gets
    /// [`Event::Views`]. A path that isn't absolute is ignored.
    SetView { folder: PathBuf, view: FolderView },
}

/// Something the folders feature's core half tells one client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Every folder given a view, newest first, answering
    /// [`Command::Views`] and after each change.
    Views(FolderViews),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_step_up_and_down_and_stop_at_the_ends() {
        assert_eq!(IconSize::Medium.bigger(), IconSize::Large);
        assert_eq!(IconSize::Medium.smaller(), IconSize::Small);
        assert_eq!(IconSize::Largest.bigger(), IconSize::Largest);
        assert_eq!(IconSize::Smallest.smaller(), IconSize::Smallest);
        assert_eq!(IconSize::Smallest.bigger(), IconSize::Small);
        assert_eq!(IconSize::Largest.smaller(), IconSize::Large);
        assert_eq!(IconSize::Smallest.stepped(3), IconSize::Large);
        assert_eq!(IconSize::Large.stepped(-3), IconSize::Smallest);
        assert_eq!(IconSize::Small.stepped(9), IconSize::Largest);
        assert_eq!(IconSize::Large.stepped(-9), IconSize::Smallest);
        for (at, size) in IconSize::ALL.into_iter().enumerate() {
            assert_eq!(size.index(), at);
        }
    }
}
