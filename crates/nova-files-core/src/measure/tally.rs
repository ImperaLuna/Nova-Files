//! The running totals of a scan, one slot per thing measured, added to by
//! every walking thread at once.
//!
//! Each slot keeps its counts in atomics, so threads add without a lock,
//! and marks itself changed, so a report can send only the slots that
//! moved since the one before (see [`Tally::take_changed`]).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use nova_files_api::Size;

/// The totals of every slot.
pub struct Tally {
    slots: Box<[Slot]>,
}

#[derive(Default)]
struct Slot {
    bytes: AtomicU64,
    on_disk: AtomicU64,
    files: AtomicU64,
    folders: AtomicU64,
    unreadable: AtomicU64,
    changed: AtomicBool,
}

impl Slot {
    fn size(&self) -> Size {
        Size {
            bytes: self.bytes.load(Ordering::Relaxed),
            on_disk: self.on_disk.load(Ordering::Relaxed),
            files: self.files.load(Ordering::Relaxed),
            folders: self.folders.load(Ordering::Relaxed),
            unreadable: self.unreadable.load(Ordering::Relaxed),
        }
    }
}

impl Tally {
    /// `slots` slots, all at zero, and all changed: the first report
    /// names every slot, even one nothing is ever added to.
    pub fn new(slots: usize) -> Self {
        let fresh = || Slot {
            changed: AtomicBool::new(true),
            ..Slot::default()
        };
        Self {
            slots: (0..slots).map(|_| fresh()).collect(),
        }
    }

    /// One more file (or link, or anything not a folder) in `slot`.
    pub fn file(&self, slot: usize, bytes: u64, on_disk: u64) {
        if let Some(slot) = self.slots.get(slot) {
            slot.bytes.fetch_add(bytes, Ordering::Relaxed);
            slot.on_disk.fetch_add(on_disk, Ordering::Relaxed);
            slot.files.fetch_add(1, Ordering::Relaxed);
            slot.changed.store(true, Ordering::Relaxed);
        }
    }

    /// One more folder in `slot`, taking `on_disk` bytes itself.
    pub fn folder(&self, slot: usize, on_disk: u64) {
        if let Some(slot) = self.slots.get(slot) {
            slot.on_disk.fetch_add(on_disk, Ordering::Relaxed);
            slot.folders.fetch_add(1, Ordering::Relaxed);
            slot.changed.store(true, Ordering::Relaxed);
        }
    }

    /// One more folder in `slot` that couldn't be read.
    pub fn unreadable(&self, slot: usize) {
        if let Some(slot) = self.slots.get(slot) {
            slot.unreadable.fetch_add(1, Ordering::Relaxed);
            slot.changed.store(true, Ordering::Relaxed);
        }
    }

    /// All slots added up.
    pub fn total(&self) -> Size {
        self.slots
            .iter()
            .fold(Size::default(), |total, slot| total.plus(slot.size()))
    }

    /// Each slot that changed since the last call, with its totals now.
    pub fn take_changed(&self) -> Vec<(usize, Size)> {
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.changed.swap(false, Ordering::Relaxed))
            .map(|(index, slot)| (index, slot.size()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_slot_adds_up_on_its_own_and_the_total_adds_them_all() {
        let tally = Tally::new(2);

        tally.file(0, 10, 4096);
        tally.file(0, 5, 0);
        tally.folder(1, 4096);
        tally.unreadable(1);
        tally.file(7, 1, 1);

        let first = Size {
            bytes: 15,
            on_disk: 4096,
            files: 2,
            ..Size::default()
        };
        let second = Size {
            on_disk: 4096,
            folders: 1,
            unreadable: 1,
            ..Size::default()
        };
        assert_eq!(tally.take_changed(), vec![(0, first), (1, second)]);
        assert_eq!(tally.total(), first.plus(second));
    }

    #[test]
    fn only_slots_that_moved_since_the_last_take_come_back() {
        let tally = Tally::new(3);
        assert_eq!(tally.take_changed().len(), 3);
        tally.file(0, 1, 1);
        tally.take_changed();

        tally.folder(2, 0);
        tally.unreadable(1);

        let changed: Vec<usize> = tally.take_changed().into_iter().map(|(i, _)| i).collect();
        assert_eq!(changed, vec![1, 2]);
        assert_eq!(tally.take_changed(), Vec::new());
    }
}
