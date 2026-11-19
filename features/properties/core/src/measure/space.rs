use std::path::Path;

use nova_files_api::{Entry, Unreadable};
use nova_files_properties_api::{Event, MeasureId, ScanState, Share};

use super::Scan;
use super::tally::Tally;
use super::walk::{Root, device_of};
use nova_files_entries_core::list::{self, Stopped};

pub fn run(scan: &Scan, id: MeasureId, folder: &Path) {
    let unreadable = |reason| {
        scan.last(|state| match state {
            ScanState::CutShort => Event::SpaceMeasured {
                id,
                shares: Vec::new(),
                state,
            },
            _ => Event::SpaceUnreadable { id, reason },
        });
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
    use std::sync::Arc;
    use std::sync::mpsc;

    use nova_files_api::{ClientId, EntryId, EntryKind};
    use nova_files_properties_api::Size;

    use super::*;
    use crate::measure::{CUT, Measurer, Purpose, RUNNING, Shared};

    fn space_in(state: u8, folder: &Path) -> Vec<Event> {
        let (sink, events) = mpsc::channel();
        let shared = Arc::new(Shared::new(MeasureId(7), Vec::new()));
        shared.halt(state);
        let scan = Scan {
            scans: Arc::clone(&Measurer::default().scans),
            key: (ClientId(1), Purpose::Space),
            shared,
            events: Arc::new(sink),
        };

        run(&scan, MeasureId(7), folder);

        events.try_iter().collect()
    }

    #[test]
    fn a_folder_that_cant_be_read_says_so_unless_the_scan_was_cut_short() {
        let relative = Path::new("relative");

        assert_eq!(
            space_in(CUT, relative),
            vec![Event::SpaceMeasured {
                id: MeasureId(7),
                shares: Vec::new(),
                state: ScanState::CutShort,
            }]
        );
        assert_eq!(
            space_in(RUNNING, relative),
            vec![Event::SpaceUnreadable {
                id: MeasureId(7),
                reason: Unreadable::Failed,
            }]
        );
    }

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
