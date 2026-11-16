//! How many of a transfer's items still to come will meet a name clash,
//! so the question about one can say how many an answer for all of its
//! kind settles ("For all 4").
//!
//! The plan is looked at once, at the job's first clash: each item whose
//! target name is taken then is noted with its kind of clash. A count is
//! what the job could see at that moment, not a promise: a folder skipped
//! or kept under a new name takes what is inside it out of the way, and
//! another program may change a folder meanwhile.

use nova_files_api::ConflictKind;

use super::clash::clash;
use super::control::Control;
use super::plan::Item;

/// What a transfer knows of the clashes ahead of it.
pub struct Ahead<'a> {
    items: &'a [Item],
    /// For each kind of clash, the places in the plan, in order, of the
    /// items whose target was taken that way when first looked; `None`
    /// until a clash needs it.
    found: Option<Vec<(ConflictKind, Vec<usize>)>>,
}

impl<'a> Ahead<'a> {
    /// Nothing looked at yet, for the transfer of `items`.
    pub const fn new(items: &'a [Item]) -> Self {
        Self { items, found: None }
    }

    /// How many items after the `at`th will meet a clash of `kind`. Only
    /// a taken name counts: other conflicts (a full drive, a gone source)
    /// can't be seen before the job gets there. A cancel stops the look
    /// short; the job ends then anyway.
    pub fn more(&mut self, at: usize, kind: ConflictKind, control: &Control) -> u64 {
        if !is_name_clash(kind) {
            return 0;
        }
        let items = self.items;
        let found = self.found.get_or_insert_with(|| look(items, control));
        let more = found
            .iter()
            .find(|(found, _)| *found == kind)
            .map_or(0, |(_, places)| {
                places.len() - places.partition_point(|&index| index <= at)
            });
        u64::try_from(more).unwrap_or(u64::MAX)
    }
}

/// Whether `kind` is about a name already taken at the target.
const fn is_name_clash(kind: ConflictKind) -> bool {
    matches!(
        kind,
        ConflictKind::FileExists
            | ConflictKind::FolderExists
            | ConflictKind::KindMismatch
            | ConflictKind::SameFile
    )
}

/// Each of `items` whose target is taken, by how, in plan order.
fn look(items: &[Item], control: &Control) -> Vec<(ConflictKind, Vec<usize>)> {
    let mut found: Vec<(ConflictKind, Vec<usize>)> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if control.checkpoint().is_err() {
            break;
        }
        // One look for a free name, as most are.
        if item.to.symlink_metadata().is_err() {
            continue;
        }
        if let Ok(Some(kind)) = clash(&item.from, item.kind, &item.to)
            && is_name_clash(kind)
        {
            match found.iter_mut().find(|(found, _)| *found == kind) {
                Some((_, places)) => places.push(index),
                None => found.push((kind, vec![index])),
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use crate::ops::plan::{Entry, Kind};
    use crate::ops::test_folder::TestFolder;

    fn item(from: &Path, to: &Path, kind: Kind) -> Item {
        Item {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            kind,
            entry: Entry::default(),
        }
    }

    #[test]
    fn counts_the_later_items_whose_names_are_taken_by_kind() {
        let folder = TestFolder::new("ops-ahead");
        let file = Kind::File { size: 1 };
        let sources = folder.path().join("sources");
        let into = folder.path().join("into");
        fs::create_dir_all(sources.join("e")).unwrap();
        fs::create_dir_all(into.join("e")).unwrap();
        for name in ["a", "b", "c", "d"] {
            folder.file(&format!("sources/{name}"), name);
        }
        for name in ["a", "b", "d"] {
            folder.file(&format!("into/{name}"), "there");
        }
        let items: Vec<_> = ["a", "b", "c", "d"]
            .iter()
            .map(|name| item(&sources.join(name), &into.join(name), file))
            .chain([item(&sources.join("e"), &into.join("e"), Kind::Folder)])
            .collect();
        let control = Control::default();
        let mut ahead = Ahead::new(&items);

        assert_eq!(ahead.more(0, ConflictKind::FileExists, &control), 2);
        assert_eq!(ahead.more(1, ConflictKind::FileExists, &control), 1);
        assert_eq!(ahead.more(3, ConflictKind::FileExists, &control), 0);
        assert_eq!(ahead.more(0, ConflictKind::FolderExists, &control), 1);
        // Seen as it was first looked at.
        fs::remove_file(into.join("d")).unwrap();
        assert_eq!(ahead.more(1, ConflictKind::FileExists, &control), 1);
    }

    #[test]
    fn only_a_taken_name_is_counted_ahead() {
        let folder = TestFolder::new("ops-ahead-other");
        let from = folder.file("a", "a");
        let to = folder.file("b", "b");
        let items = [item(&from, &to, Kind::File { size: 1 })];
        let control = Control::default();
        let mut ahead = Ahead::new(&items);

        assert_eq!(ahead.more(0, ConflictKind::NoSpace, &control), 0);
        assert!(ahead.found.is_none(), "nothing looked at for it");
    }

    #[test]
    fn a_cancel_stops_the_look() {
        let folder = TestFolder::new("ops-ahead-cancel");
        let from = folder.file("a", "a");
        let to = folder.file("b", "b");
        let items = [
            item(&from, &to, Kind::File { size: 1 }),
            item(&from, &to, Kind::File { size: 1 }),
        ];
        let control = Control::default();
        control.cancel();
        let mut ahead = Ahead::new(&items);

        assert_eq!(ahead.more(0, ConflictKind::FileExists, &control), 0);
    }
}
