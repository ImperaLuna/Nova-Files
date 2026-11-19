use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use nova_files::{Home, IDLE_EXIT, IndexPlace};
use nova_files_api::{
    ClientGone, Command, Connection, EntryId, EntryKind, Envelope, Event, EventSink, Listing,
    ListingId, Order, ThumbnailSize,
};
use nova_files_search_api::{
    Command as SearchCommand, Event as SearchEvent, Hit, MAX_HITS, Matched, SearchFailed, SearchId,
};

use crate::common::Core;

const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);
const CHANGE_TIMEOUT: Duration = Duration::from_secs(5);
const LIMIT: u32 = 50;

struct Scratch(PathBuf);

impl Scratch {
    fn new(files: &[&str]) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "nova-files-core-search-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let scratch = Self(std::env::temp_dir().join(name));
        fs::create_dir_all(scratch.home()).unwrap();
        for file in files {
            scratch.add(file);
        }
        scratch
    }

    fn home(&self) -> PathBuf {
        self.0.join("home")
    }

    fn place(&self) -> IndexPlace {
        IndexPlace {
            folder: self.0.join("cache/nova-files"),
        }
    }

    fn add(&self, file: &str) {
        let path = self.home().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"").unwrap();
    }

    fn index_inode(&self) -> u64 {
        fs::metadata(self.place().index()).unwrap().ino()
    }

    fn core(&self, place: Option<IndexPlace>) -> Client {
        let home = Home {
            folder: self.home(),
            config: self.0.join("config"),
            state: self.0.join("state"),
            data: self.0.join("share"),
            drive_trashes: false,
        };
        let core = Core(nova_files::spawn_with(IDLE_EXIT, Some(home), None, place).unwrap());
        let (sink, events) = mpsc::channel();
        let connection = core.connector.connect(Both(sink)).unwrap();
        Client {
            _core: core,
            connection,
            events,
            home: self.home(),
            next: 0,
        }
    }
}

enum Heard {
    Folders(Event),
    Search(SearchEvent),
}

struct Both(Sender<Heard>);

impl EventSink for Both {
    fn deliver_envelope(&self, envelope: Envelope) -> Result<(), ClientGone> {
        let heard = match envelope.open::<Event>() {
            Ok(event) => Heard::Folders(event),
            Err(other) => match other.open::<SearchEvent>() {
                Ok(event) => Heard::Search(event),
                Err(_) => return Ok(()),
            },
        };
        self.0.send(heard).map_err(|_| ClientGone)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

struct Client {
    _core: Core,
    connection: Connection,
    events: Receiver<Heard>,
    home: PathBuf,
    next: u64,
}

impl Client {
    fn send(&mut self, query: &str, limit: u32) -> SearchId {
        self.next += 1;
        let id = SearchId(self.next);
        let query = query.to_owned();
        self.connection
            .send(SearchCommand::Search { id, query, limit })
            .unwrap();
        id
    }

    fn find(&mut self, query: &str, within: Option<PathBuf>) -> ListingId {
        self.next += 1;
        let id = ListingId(self.next);
        let query = query.to_owned();
        self.connection
            .send(SearchCommand::Find { id, query, within })
            .unwrap();
        id
    }

    fn found(&mut self, query: &str, within: Option<PathBuf>) -> Listing {
        let asked = self.find(query, within);
        let Event::FolderListed(listing) = self.answer() else {
            panic!("expected a listing");
        };
        assert_eq!(listing.id, asked);
        listing
    }

    fn found_paths(&self, listing: &Listing) -> Vec<(PathBuf, EntryKind)> {
        listing
            .entries
            .iter()
            .map(|entry| {
                let path = listing.path_of(entry);
                (
                    path.strip_prefix(&self.home).unwrap().to_owned(),
                    entry.kind,
                )
            })
            .collect()
    }

    fn answer(&self) -> Event {
        match self.events.recv_timeout(ANSWER_TIMEOUT).unwrap() {
            Heard::Folders(event) => event,
            Heard::Search(event) => panic!("expected a folder event, got {event:?}"),
        }
    }

    fn searched(&self) -> SearchEvent {
        match self.events.recv_timeout(ANSWER_TIMEOUT).unwrap() {
            Heard::Search(event) => event,
            Heard::Folders(event) => panic!("expected a search event, got {event:?}"),
        }
    }

    fn stages(&mut self, query: &str, limit: u32) -> Vec<Vec<(PathBuf, Matched)>> {
        let asked = self.send(query, limit);
        let mut stages = Vec::new();
        loop {
            let SearchEvent::Found { id, hits, done } = self.searched() else {
                panic!("expected hits");
            };
            assert_eq!(id, asked);
            stages.push(hits.into_iter().map(|hit| self.named(&hit)).collect());
            if done {
                return stages;
            }
        }
    }

    fn named(&self, hit: &Hit) -> (PathBuf, Matched) {
        let path = hit.path.strip_prefix(&self.home).unwrap().to_owned();
        (path, hit.matched)
    }

    fn first_stage_becomes(&mut self, query: &str, wanted: &[&str]) {
        let wanted: Vec<PathBuf> = wanted.iter().map(PathBuf::from).collect();
        let started = Instant::now();
        loop {
            let first = self.stages(query, LIMIT).remove(0);
            let paths: Vec<PathBuf> = first.into_iter().map(|(path, _)| path).collect();
            if paths == wanted {
                return;
            }
            assert!(started.elapsed() < CHANGE_TIMEOUT, "still {paths:?}");
        }
    }
}

fn at(path: &str, matched: Matched) -> (PathBuf, Matched) {
    (PathBuf::from(path), matched)
}

#[test]
fn the_first_search_indexes_home_and_answers_in_stages() {
    let scratch = Scratch::new(&["Documents/report.pdf", "Documents/repart.txt"]);
    let mut client = scratch.core(Some(scratch.place()));
    assert_eq!(
        client.stages("report", LIMIT),
        [
            vec![at("Documents/report.pdf", Matched::Exact)],
            vec![at("Documents/repart.txt", Matched::OneTypo)],
        ]
    );
    assert!(scratch.place().index().exists());
    assert_eq!(
        client.stages("rprt", LIMIT),
        [
            vec![],
            vec![
                at("Documents/repart.txt", Matched::InOrder),
                at("Documents/report.pdf", Matched::InOrder),
            ],
        ]
    );
}

#[test]
fn files_made_or_removed_since_are_seen() {
    let scratch = Scratch::new(&["notes/old plan.txt"]);
    let mut client = scratch.core(Some(scratch.place()));
    client.first_stage_becomes("plan", &["notes/old plan.txt"]);
    scratch.add("notes/new plan.txt");
    fs::remove_file(scratch.home().join("notes/old plan.txt")).unwrap();
    client.first_stage_becomes("plan", &["notes/new plan.txt"]);
}

#[test]
fn a_saved_index_of_home_is_reused() {
    let scratch = Scratch::new(&["a/report.pdf"]);
    scratch.place().build(&scratch.home()).unwrap();
    let saved = scratch.index_inode();
    scratch.add("a/report 2.pdf");
    let mut client = scratch.core(Some(scratch.place()));
    client.first_stage_becomes("report", &["a/report.pdf", "a/report 2.pdf"]);
    assert_eq!(scratch.index_inode(), saved);
}

#[test]
fn a_saved_index_of_another_folder_is_replaced() {
    let scratch = Scratch::new(&["a/report.pdf"]);
    let elsewhere = scratch.0.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    scratch.place().build(&elsewhere).unwrap();
    let saved = scratch.index_inode();
    let mut client = scratch.core(Some(scratch.place()));
    client.first_stage_becomes("report", &["a/report.pdf"]);
    assert_ne!(scratch.index_inode(), saved);
}

#[test]
fn nothing_to_look_for_answers_once_with_nothing() {
    let scratch = Scratch::new(&["a/report.pdf"]);
    let mut client = scratch.core(Some(scratch.place()));
    let none: Vec<Vec<(PathBuf, Matched)>> = vec![vec![]];
    assert_eq!(client.stages("  ", LIMIT), none);
    assert_eq!(client.stages("report", 0), none);
}

#[test]
fn a_stage_holds_at_most_max_hits() {
    let names: Vec<String> = (0..=MAX_HITS).map(|n| format!("file {n}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let scratch = Scratch::new(&names);
    let mut client = scratch.core(Some(scratch.place()));
    let first = client.stages("file", u32::MAX).remove(0);
    assert_eq!(first.len(), usize::try_from(MAX_HITS).unwrap());
}

#[test]
fn without_a_cache_folder_search_fails() {
    let scratch = Scratch::new(&[]);
    let mut client = scratch.core(None);
    let id = client.send("report", LIMIT);
    let reason = SearchFailed::NoIndex;
    assert_eq!(client.searched(), SearchEvent::SearchFailed { id, reason });
}

#[test]
fn an_index_that_cant_be_saved_fails() {
    let scratch = Scratch::new(&["a/report.pdf"]);
    let blocked = scratch.0.join("blocked");
    fs::write(&blocked, b"").unwrap();
    let place = IndexPlace {
        folder: blocked.join("nova-files"),
    };
    let mut client = scratch.core(Some(place));
    let id = client.send("report", LIMIT);
    let reason = SearchFailed::IndexFailed;
    assert_eq!(client.searched(), SearchEvent::SearchFailed { id, reason });
}

#[test]
fn found_files_are_a_listing_best_first_with_every_stage() {
    let scratch = Scratch::new(&["Documents/report.pdf", "Old/repart.txt"]);
    fs::create_dir_all(scratch.home().join("Reports/report")).unwrap();
    let mut client = scratch.core(Some(scratch.place()));
    let listing = client.found("report", None);
    assert_eq!(listing.path, scratch.home());
    assert_eq!(
        client.found_paths(&listing),
        [
            (PathBuf::from("Reports/report"), EntryKind::Folder),
            (PathBuf::from("Documents/report.pdf"), EntryKind::File),
            (PathBuf::from("Reports"), EntryKind::Folder),
            (PathBuf::from("Old/repart.txt"), EntryKind::File),
        ]
    );
}

#[test]
fn found_files_within_a_folder_are_only_those_inside_it() {
    let scratch = Scratch::new(&["Documents/a/report.pdf", "Old/report.txt"]);
    let mut client = scratch.core(Some(scratch.place()));
    let documents = scratch.home().join("Documents");
    let listing = client.found("report", Some(documents.clone()));
    assert_eq!(listing.path, documents);
    assert_eq!(
        client.found_paths(&listing),
        [(PathBuf::from("Documents/a/report.pdf"), EntryKind::File)]
    );
}

#[test]
fn rows_of_found_files_get_their_details() {
    let scratch = Scratch::new(&["Documents/report.pdf"]);
    fs::write(scratch.home().join("Documents/report.pdf"), b"12345").unwrap();
    let mut client = scratch.core(Some(scratch.place()));
    let listing = client.found("report", None);
    let rows = Command::Rows {
        id: listing.id,
        first: EntryId(0),
        count: 1,
        size: ThumbnailSize::Normal,
    };
    client.connection.send(rows).unwrap();
    let Event::DetailsRead { id, details, .. } = client.answer() else {
        panic!("expected details");
    };
    assert_eq!((id, details[0].size), (listing.id, Some(5)));
}

#[test]
fn a_folder_asked_for_after_a_find_replaces_it() {
    let scratch = Scratch::new(&["Documents/report.pdf"]);
    let mut client = scratch.core(Some(scratch.place()));
    client.found("report", None);
    client.find("report", None);
    client.next += 1;
    let folder = ListingId(client.next);
    let list = Command::ListFolder {
        id: folder,
        path: scratch.home(),
        order: Order::default(),
    };
    client.connection.send(list).unwrap();
    let started = Instant::now();
    loop {
        let Event::FolderListed(listing) = client.answer() else {
            panic!("expected a listing");
        };
        if listing.id == folder {
            break;
        }
        assert!(started.elapsed() < ANSWER_TIMEOUT);
    }
}

#[test]
fn without_a_cache_folder_find_fails() {
    let scratch = Scratch::new(&[]);
    let mut client = scratch.core(None);
    let id = client.find("report", None);
    let reason = SearchFailed::NoIndex;
    assert_eq!(client.searched(), SearchEvent::FindFailed { id, reason });
}
