use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};

use nova_files_api::{FileIcon, IconKind, SortKey};
use nova_files_disk::ports::Icons;

use super::*;
use crate::client::Client;

const QUIET: Duration = Duration::from_millis(300);
const BIG: u64 = 900;
const SMALL: u64 = 5;

struct Plain;

impl Icons for Plain {
    fn of(&self, _: &std::ffi::OsStr, _: EntryKind) -> FileIcon {
        FileIcon::BuiltIn(IconKind::Folder)
    }

    fn follow_desktop(&self, _: &[String]) {}
}

struct Fixed {
    asked: AtomicUsize,
}

impl MeasureFolders for Fixed {
    fn measure(
        &self,
        _: &Path,
        folders: &[OsString],
        _: &(dyn Fn() -> bool + Sync),
        told: Told<'_>,
    ) -> Option<Vec<u64>> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let bytes = |name: &OsString| if name == "big" { BIG } else { SMALL };
        told(vec![(0, 1)]);
        Some(folders.iter().map(bytes).collect())
    }
}

struct Listed {
    names: Vec<String>,
    sizes: Vec<Option<u64>>,
    measured: Vec<Event>,
}

fn list(client: &Client, sizer: &FolderSizer, id: u64, path: &Path) {
    let job = client.start_listing(ListingId(id));
    let entries = crate::list::read_folder(path, || false).unwrap();
    let order = Order {
        key: SortKey::Size,
        descending: true,
    };
    list_measured(&job, (ListingId(id), path.into()), entries, order, sizer);
}

fn take(events: &Receiver<Event>) -> Listed {
    let mut listed = Listed {
        names: Vec::new(),
        sizes: Vec::new(),
        measured: Vec::new(),
    };
    while let Ok(event) = events.recv_timeout(QUIET) {
        match event {
            Event::FolderListed(listing) => {
                listed.names = listing
                    .entries
                    .iter()
                    .map(|entry| entry.name.to_string_lossy().into_owned())
                    .collect();
            }
            Event::DetailsRead { details, .. } => {
                listed.sizes = details.iter().map(|details| details.size).collect();
            }
            event @ Event::FoldersMeasured { .. } => listed.measured.push(event),
            _ => {}
        }
    }
    listed
}

#[test]
fn folders_are_measured_then_sorted_with_files_by_what_they_hold() {
    let root = std::env::temp_dir().join(format!("nova-files-sizes-{}", std::process::id()));
    fs::remove_dir_all(&root).ok();
    fs::create_dir_all(root.join("big")).unwrap();
    fs::create_dir_all(root.join("small")).unwrap();
    fs::write(root.join("file"), [0_u8; 50]).unwrap();
    let (sender, events) = mpsc::channel::<Event>();
    let icons: Arc<dyn Icons> = Arc::new(Plain);
    let client = Client::new(Arc::new(sender), None, &icons);
    let sizer = FolderSizer::new(Arc::new(Fixed {
        asked: AtomicUsize::new(0),
    }));

    list(&client, &sizer, 1, &root);
    let first = take(&events);
    list(&client, &sizer, 2, &root);
    client.want_rows(ListingId(2), EntryId(0), 3, None);
    let again = take(&events);

    fs::remove_dir_all(&root).ok();
    assert_eq!(first.names, ["big", "small", "file"]);
    assert_eq!(
        first.measured,
        [
            Event::FoldersMeasured {
                id: ListingId(1),
                sizes: vec![(EntryId(0), 1)],
                done: false,
            },
            Event::FoldersMeasured {
                id: ListingId(1),
                sizes: vec![(EntryId(0), 900), (EntryId(1), 5)],
                done: true,
            },
        ]
    );
    assert_eq!(again.names, ["big", "file", "small"]);
    assert_eq!(again.sizes, [Some(900), Some(50), Some(5)]);
    assert!(again.measured.is_empty(), "{:?}", again.measured);
}

#[test]
fn a_folder_the_last_measure_missed_is_measured_again() {
    let root = std::env::temp_dir().join(format!("nova-files-sizes-new-{}", std::process::id()));
    fs::remove_dir_all(&root).ok();
    fs::create_dir_all(root.join("big")).unwrap();
    let (sender, events) = mpsc::channel::<Event>();
    let icons: Arc<dyn Icons> = Arc::new(Plain);
    let client = Client::new(Arc::new(sender), None, &icons);
    let sizer = FolderSizer::new(Arc::new(Fixed {
        asked: AtomicUsize::new(0),
    }));

    list(&client, &sizer, 1, &root);
    take(&events);
    fs::create_dir_all(root.join("new")).unwrap();
    list(&client, &sizer, 2, &root);
    let again = take(&events);

    fs::remove_dir_all(&root).ok();
    assert!(
        again
            .measured
            .iter()
            .any(|event| matches!(event, Event::FoldersMeasured { done: true, .. })),
        "{:?}",
        again.measured
    );
}

#[test]
fn the_oldest_folder_is_forgotten_past_the_cap() {
    let sizer = FolderSizer::new(Arc::new(Fixed {
        asked: AtomicUsize::new(0),
    }));
    for at in 0..=KEPT_FOLDERS {
        sizer.keep(PathBuf::from(format!("/f{at}")), HashMap::new());
    }

    assert!(sizer.known(Path::new("/f0")).is_none());
    assert!(
        sizer
            .known(Path::new(&format!("/f{KEPT_FOLDERS}")))
            .is_some()
    );
}
