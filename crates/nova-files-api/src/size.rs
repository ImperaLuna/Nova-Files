//! How much space files and folders take, as the core measures it.
//!
//! A client asks with [`Command::Measure`], naming the scan with a
//! [`MeasureId`] of its own choosing. The core walks everything under the
//! paths and answers that client alone with [`Event::SizeMeasured`] a few
//! times a second, the totals so far ([`ScanState::Counting`]), then once
//! more when it ends ([`ScanState::Done`], or [`ScanState::CutShort`] when
//! the core had to stop it, say for a drive about to be unmounted).
//! [`Command::StopMeasure`] ends a scan early, and so do a newer scan from
//! the same client and the client leaving.
//!
//! A scan never follows a link and never goes into another drive mounted
//! inside the folder. A folder or a file reached twice (two paths, one
//! inside the other, or a file with several hard links) counts once.
//!
//! [`Command::Measure`]: crate::Command::Measure
//! [`Command::StopMeasure`]: crate::Command::StopMeasure
//! [`Event::SizeMeasured`]: crate::Event::SizeMeasured

/// Names one scan. The client picks it; the core echoes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MeasureId(pub u64);

/// Where a scan is, as of an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScanState {
    /// Still counting; more answers follow.
    Counting,
    /// Everything was counted; the last answer.
    Done,
    /// The core stopped it before the end, so the totals miss some; the
    /// last answer.
    CutShort,
}

/// What a scan has counted so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size {
    /// The files' and links' lengths added up, as a file's size shows.
    pub bytes: u64,
    /// What they take on the drive, folders included: less than `bytes`
    /// for sparse or compressed files, more for many small ones.
    pub on_disk: u64,
    /// Files, links and anything else that isn't a folder.
    pub files: u64,
    pub folders: u64,
    /// Folders, entries or named paths that couldn't be read (or a path
    /// that isn't absolute), so the totals miss what they hold.
    pub unreadable: u64,
}

impl Size {
    /// The two totals added up, each field on its own.
    #[must_use]
    pub const fn plus(self, other: Self) -> Self {
        Self {
            bytes: self.bytes.saturating_add(other.bytes),
            on_disk: self.on_disk.saturating_add(other.on_disk),
            files: self.files.saturating_add(other.files),
            folders: self.folders.saturating_add(other.folders),
            unreadable: self.unreadable.saturating_add(other.unreadable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plus_adds_each_field_and_stops_at_the_top() {
        let one = Size {
            bytes: 1,
            on_disk: 2,
            files: 3,
            folders: 4,
            unreadable: 5,
        };
        let full = Size {
            bytes: u64::MAX,
            ..Size::default()
        };

        assert_eq!(
            one.plus(one),
            Size {
                bytes: 2,
                on_disk: 4,
                files: 6,
                folders: 8,
                unreadable: 10,
            }
        );
        assert_eq!(full.plus(one).bytes, u64::MAX);
    }
}
