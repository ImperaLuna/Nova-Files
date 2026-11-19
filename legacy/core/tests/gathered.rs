#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

#[expect(dead_code, reason = "this file starts no jobs")]
mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use std::sync::mpsc::{self, Receiver, Sender};

use common::{ANSWER_TIMEOUT, Bench};
use nova_files_api::{
    ClientGone, Command, Connection, EntryId, Envelope, Event, EventSink, Listing, ListingId,
    Message, Order, ThumbnailSize,
};
use nova_files_gathered_api::{self as gathered_api, Gathered, GatheredChange, Gathering, Holds};

struct Envelopes(Sender<Envelope>);

impl EventSink for Envelopes {
    fn deliver_envelope(&self, envelope: Envelope) -> Result<(), ClientGone> {
        self.0.send(envelope).map_err(|_| ClientGone)
    }
}

#[derive(Debug)]
enum Heard {
    Gathered(gathered_api::Event),
    Core(Event),
}

struct Client {
    connection: Connection,
    events: Receiver<Envelope>,
}

impl Client {
    fn of(bench: &Bench) -> Self {
        let (sink, events) = mpsc::channel();
        let connection = bench.core.connector.connect(Envelopes(sink)).unwrap();
        Self { connection, events }
    }

    fn send(&self, command: impl Message) {
        self.connection.send(command).unwrap();
    }

    fn next(&self) -> Heard {
        let envelope = self.events.recv_timeout(ANSWER_TIMEOUT).unwrap();
        match envelope.open::<gathered_api::Event>() {
            Ok(event) => Heard::Gathered(event),
            Err(envelope) => Heard::Core(envelope.open::<Event>().unwrap()),
        }
    }
}

fn gathered_file(bench: &Bench) -> PathBuf {
    bench.at("home/.config/nova-files/gathered.toml")
}

fn next(client: &Client) -> Arc<Gathered> {
    match client.next() {
        Heard::Gathered(gathered_api::Event::Gathered(gathered)) => gathered,
        other => panic!("expected the gathered folders, got {other:?}"),
    }
}

fn paths(gathered: &Gathered, place: Gathering) -> Vec<&Path> {
    gathered
        .of(place)
        .iter()
        .map(|folder| folder.path.as_path())
        .collect()
}

#[test]
fn a_first_start_gathers_projects_and_steam() {
    let bench = Bench::with("gathered-first", nova_files_core::IDLE_EXIT, |root| {
        fs::create_dir_all(root.join("home/Projects/nova")).unwrap();
        fs::create_dir_all(root.join("home/.local/share/Steam/steamapps/common")).unwrap();
    });
    let client = Client::of(&bench);
    client.send(gathered_api::Command::Gathered);
    let gathered = next(&client);
    assert_eq!(
        paths(&gathered, Gathering::Projects),
        [bench.at("home/Projects")]
    );
    assert_eq!(
        paths(&gathered, Gathering::Games),
        [bench.at("home/.local/share/Steam/steamapps/common")]
    );
    assert_eq!(gathered.games[0].name.as_deref(), Some("Steam"));
    let text = fs::read_to_string(gathered_file(&bench)).unwrap();
    assert!(text.contains("\"~/Projects\""), "{text}");
}

#[test]
fn a_change_reaches_every_window_and_the_file() {
    let bench = Bench::new("gathered-change");
    let skills = bench.at("home/Skills");
    fs::create_dir_all(skills.join(".git")).unwrap();
    let window = Client::of(&bench);
    let other = Client::of(&bench);
    window.send(gathered_api::Command::Gathered);
    other.send(gathered_api::Command::Gathered);
    assert_eq!(*next(&window), Gathered::default());
    assert_eq!(*next(&other), Gathered::default());

    window.send(gathered_api::Command::ChangeGathered(GatheredChange::Add {
        place: Gathering::Projects,
        path: skills.clone(),
        holds: None,
        at: None,
        name: None,
    }));
    let gathered = next(&window);
    assert_eq!(gathered.projects[0].holds, Holds::One);
    assert_eq!(next(&other), gathered);
    let text = fs::read_to_string(gathered_file(&bench)).unwrap();
    assert!(text.contains("\"~/Skills\""), "{text}");

    window.send(gathered_api::Command::ChangeGathered(GatheredChange::Add {
        place: Gathering::Projects,
        path: bench.at("home/Work"),
        holds: Some(Holds::Many),
        at: None,
        name: None,
    }));
    next(&window);
    window.send(gathered_api::Command::ChangeGathered(
        GatheredChange::Remove {
            place: Gathering::Projects,
            path: skills.clone(),
        },
    ));
    assert_eq!(
        paths(&next(&window), Gathering::Projects),
        [bench.at("home/Work")]
    );
    window.send(gathered_api::Command::ChangeGathered(GatheredChange::Add {
        place: Gathering::Projects,
        path: skills.clone(),
        holds: Some(Holds::One),
        at: Some(0),
        name: None,
    }));
    assert_eq!(
        paths(&next(&window), Gathering::Projects),
        [skills, bench.at("home/Work")]
    );
}

#[test]
fn the_file_edited_by_hand_reaches_the_windows() {
    let bench = Bench::new("gathered-by-hand");
    let client = Client::of(&bench);
    client.send(gathered_api::Command::Gathered);
    next(&client);
    let file = gathered_file(&bench);
    let temporary = file.with_extension("tmp");
    fs::write(&temporary, "[[games]]\npath = \"/mnt/games\"\n").unwrap();
    fs::rename(temporary, &file).unwrap();
    assert_eq!(
        paths(&next(&client), Gathering::Games),
        [Path::new("/mnt/games")]
    );
}

fn listed(client: &Client, id: u64, place: Gathering) -> Listing {
    client.send(gathered_api::Command::ListGathered {
        id: ListingId(id),
        place,
        order: Order::default(),
    });
    loop {
        match client.next() {
            Heard::Core(Event::FolderListed(listing)) if listing.id == ListingId(id) => {
                return listing;
            }
            Heard::Gathered(_) => {}
            other @ Heard::Core(_) => panic!("expected listing {id}, got {other:?}"),
        }
    }
}

fn names(listing: &Listing) -> Vec<String> {
    listing
        .entries
        .iter()
        .map(|entry| entry.name.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn a_place_lists_and_says_when_its_folders_change() {
    let bench = Bench::with("gathered-list", nova_files_core::IDLE_EXIT, |root| {
        fs::create_dir_all(root.join("home/Projects/nova")).unwrap();
        fs::create_dir_all(root.join("home/Projects/bench")).unwrap();
        fs::write(root.join("home/Projects/notes.txt"), "").unwrap();
    });
    let client = Client::of(&bench);
    let listing = listed(&client, 1, Gathering::Projects);
    assert_eq!(names(&listing), ["bench", "nova"]);
    assert_eq!(
        listing.path_of(&listing.entries[1]),
        bench.at("home/Projects/nova")
    );
    assert_eq!(listing.path, PathBuf::new());

    client.send(Command::Rows {
        id: ListingId(1),
        first: EntryId(0),
        count: 2,
        size: ThumbnailSize::Normal,
    });
    loop {
        if let Heard::Core(Event::DetailsRead { id, .. }) = client.next() {
            assert_eq!(id, ListingId(1));
            break;
        }
    }

    client.send(gathered_api::Command::Gathered);
    next(&client);
    fs::create_dir(bench.at("home/Projects/zed")).unwrap();
    loop {
        match client.next() {
            Heard::Gathered(gathered_api::Event::GatheredChanged(Gathering::Projects)) => break,
            Heard::Core(Event::DetailsRead { .. }) => {}
            other => panic!("expected Projects to change, got {other:?}"),
        }
    }
    assert_eq!(
        names(&listed(&client, 2, Gathering::Projects)),
        ["bench", "nova", "zed"]
    );
    assert_eq!(
        names(&listed(&client, 3, Gathering::Games)),
        Vec::<String>::new()
    );
}
