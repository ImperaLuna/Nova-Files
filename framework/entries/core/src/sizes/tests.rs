use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use nova_files_api::{FileIcon, IconKind, SortKey};
use nova_files_disk::ports::Icons;
use nova_files_test_folder::TestFolder;

use super::*;
use crate::client::Client;

const QUIET: Duration = Duration::from_millis(300);
const BIG: u64 = 900;
const SMALL: u64 = 5;
const FILE: usize = 50;

struct Plain;

impl Icons for Plain {
    fn of(&self, _: &std::ffi::OsStr, _: EntryKind) -> FileIcon {
        FileIcon::BuiltIn(IconKind::Folder)
    }

    fn kind_name(&self, _: &std::ffi::OsStr, _: EntryKind) -> Option<Arc<str>> {
        None
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
    listings: Vec<Vec<String>>,
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
        listings: Vec::new(),
        sizes: Vec::new(),
        measured: Vec::new(),
    };
    while let Ok(event) = events.recv_timeout(QUIET) {
        match event {
            Event::FolderListed(listing) => {
                listed.listings.push(
                    listing
                        .entries
                        .iter()
                        .map(|entry| entry.name.to_string_lossy().into_owned())
                        .collect(),
                );
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

struct Bench {
    client: Client,
    events: Receiver<Event>,
    measure: Arc<Fixed>,
    sizer: FolderSizer,
}

impl Bench {
    fn new() -> Self {
        let (sender, events) = mpsc::channel::<Event>();
        let icons: Arc<dyn Icons> = Arc::new(Plain);
        let measure = Arc::new(Fixed {
            asked: AtomicUsize::new(0),
        });
        Self {
            client: Client::new(Arc::new(sender), None, &icons),
            events,
            sizer: FolderSizer::new(Arc::clone(&measure) as Arc<dyn MeasureFolders>),
            measure,
        }
    }
}

fn sample_folder(root: &Path) {
    fs::create_dir_all(root.join("big")).unwrap();
    fs::create_dir_all(root.join("small")).unwrap();
    fs::write(root.join("file"), [0_u8; FILE]).unwrap();
}

#[test]
fn folders_are_measured_then_sorted_with_files_by_what_they_hold() {
    let folder = TestFolder::new("sizes");
    let root = folder.path();
    sample_folder(root);
    let Bench {
        client,
        events,
        measure,
        sizer,
    } = Bench::new();

    list(&client, &sizer, 1, root);
    client.want_rows(ListingId(1), EntryId(0), 3, None);
    let listed = take(&events);

    assert_eq!(
        listed.listings,
        [["big", "small", "file"], ["big", "file", "small"]]
    );
    assert_eq!(
        listed.measured,
        [
            Event::FoldersMeasured {
                id: ListingId(1),
                sizes: vec![(EntryId(0), 1)],
            },
            Event::FoldersMeasured {
                id: ListingId(1),
                sizes: vec![(EntryId(0), BIG), (EntryId(2), SMALL)],
            },
        ]
    );
    assert_eq!(listed.sizes, [Some(900), Some(50), Some(5)]);
    assert_eq!(measure.asked.load(Ordering::SeqCst), 1);
}

#[test]
fn a_folder_listed_again_is_sorted_by_its_last_totals_and_measured_again() {
    let folder = TestFolder::new("sizes-again");
    let root = folder.path();
    sample_folder(root);
    let Bench {
        client,
        events,
        measure,
        sizer,
    } = Bench::new();

    list(&client, &sizer, 1, root);
    take(&events);
    list(&client, &sizer, 2, root);
    let again = take(&events);

    assert_eq!(
        again.listings,
        [["big", "file", "small"], ["big", "file", "small"]]
    );
    assert_eq!(measure.asked.load(Ordering::SeqCst), 2);
}

#[test]
fn the_oldest_folder_is_forgotten_past_the_cap() {
    let sizer = FolderSizer::new(Arc::new(Fixed {
        asked: AtomicUsize::new(0),
    }));
    for at in 0..=KEPT_FOLDERS {
        sizer.keep(vec![(PathBuf::from(format!("/f{at}")), HashMap::new())]);
    }

    assert!(sizer.known(Path::new("/f0")).is_none());
    assert!(
        sizer
            .known(Path::new(&format!("/f{KEPT_FOLDERS}")))
            .is_some()
    );
}

#[test]
fn gathered_folders_are_measured_where_each_one_lives() {
    let folder = TestFolder::new("sizes-rows");
    let root = folder.path();
    fs::create_dir_all(root.join("one/small")).unwrap();
    fs::create_dir_all(root.join("two/big")).unwrap();
    let Bench {
        client,
        events,
        measure,
        sizer,
    } = Bench::new();
    let order = Order {
        key: SortKey::Size,
        descending: true,
    };
    let rows = || {
        vec![
            (folder_entry("small"), root.join("one")),
            (folder_entry("big"), root.join("two")),
        ]
    };
    let listed = |id| {
        let job = client.start_listing(ListingId(id));
        let place = (ListingId(id), PathBuf::new());
        list_measured_rows(&job, place, rows(), (order, Homes::PerRow), &sizer);
    };

    listed(1);
    let first = take(&events);

    assert_eq!(measure.asked.load(Ordering::SeqCst), 2);
    assert_eq!(first.listings, [["small", "big"], ["big", "small"]]);
}

fn folder_entry(name: &str) -> Entry {
    Entry {
        id: EntryId(0),
        name: name.into(),
        kind: EntryKind::Folder,
    }
}

#[test]
fn folders_measured_together_are_all_kept_past_the_cap() {
    let sizer = FolderSizer::new(Arc::new(Fixed {
        asked: AtomicUsize::new(0),
    }));
    let many: Vec<_> = (0..=KEPT_FOLDERS)
        .map(|at| (PathBuf::from(format!("/g{at}")), HashMap::new()))
        .collect();

    sizer.keep(many);

    assert!((0..=KEPT_FOLDERS).all(|at| { sizer.known(Path::new(&format!("/g{at}"))).is_some() }));
}
