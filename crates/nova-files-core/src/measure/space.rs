//! A space scan: what each entry of one folder takes.
//!
//! [`run`] lists the folder the way a window does (see `list`), then walks
//! every entry as a root of its own, each into its own tally slot, as an
//! entry met in a walk: one that vanished since is left out, and a drive
//! mounted there isn't entered. A
//! report names the entries whose totals changed since the report before;
//! since every entry is counted before the first report, that one names
//! them all.

use std::path::Path;

use nova_files_api::{Entry, Event, MeasureId, Share, Unreadable};

use super::Scan;
use super::tally::Tally;
use super::walk::{Root, device_of};
use crate::list::{self, Stopped};

/// Runs `scan`, scan `id` of the entries of `folder`.
pub fn run(scan: &Scan, id: MeasureId, folder: &Path) {
    let unreadable = |reason| {
        scan.tell(|| Event::SpaceUnreadable { id, reason }).ok();
    };
    if !folder.is_absolute() {
        unreadable(Unreadable::Failed);
        return;
    }
    let entries = match list::read_folder(folder, || !scan.shared.running()) {
        Ok(entries) => entries,
        Err(Stopped::Unreadable(reason)) => {
            unreadable(reason);
            return;
        }
        Err(Stopped::Cancelled) => {
            // Cut short (or stopped) before any entry was counted.
            scan.last(|state| Event::SpaceMeasured {
                id,
                shares: Vec::new(),
                state,
            });
            return;
        }
    };
    let Some(device) = device_of(folder) else {
        unreadable(Unreadable::Missing);
        return;
    };
    let roots: Vec<Root> = entries
        .iter()
        .enumerate()
        .map(|(slot, entry)| Root {
            path: folder.join(&entry.name),
            slot,
            listed_in: Some(device),
        })
        .collect();
    let tally = Tally::new(entries.len());
    let report = |state| Event::SpaceMeasured {
        id,
        shares: changed(&entries, &tally),
        state,
    };
    scan.run(&roots, &tally, report);
}

/// The entries whose totals changed since the last call, with their
/// totals now.
fn changed(entries: &[Entry], tally: &Tally) -> Vec<Share> {
    tally
        .take_changed()
        .into_iter()
        .filter_map(|(slot, size)| {
            entries.get(slot).map(|entry| Share {
                name: entry.name.clone(),
                kind: entry.kind,
                size,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use nova_files_api::{EntryId, EntryKind, Size};

    use super::*;

    #[test]
    fn a_report_names_only_the_entries_that_changed() {
        let entry = |id, name: &str, kind| Entry {
            id: EntryId(id),
            name: name.into(),
            kind,
        };
        let entries = [
            entry(0, "photos", EntryKind::Folder),
            entry(1, "notes", EntryKind::File),
        ];
        let tally = Tally::new(entries.len());
        assert_eq!(changed(&entries, &tally).len(), 2);
        tally.file(1, 30, 4096);
        tally.folder(0, 4096);
        changed(&entries, &tally);

        tally.file(0, 7, 0);

        let shares = changed(&entries, &tally);
        assert_eq!(
            shares,
            vec![Share {
                name: "photos".into(),
                kind: EntryKind::Folder,
                size: Size {
                    bytes: 7,
                    on_disk: 4096,
                    files: 1,
                    folders: 1,
                    unreadable: 0,
                },
            }]
        );
    }
}
