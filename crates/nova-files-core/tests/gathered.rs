//! Games and Projects' folders through the real core, in a temp home: the
//! usual folders on first start, changes from a client saved to
//! `gathered.toml` and told to every window, the file edited by hand, and
//! a place listed and listed again when its folders change.

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

use common::{Bench, Client};
use nova_files_api::{
    Command, EntryId, Event, Gathered, GatheredChange, Gathering, Holds, Listing, ListingId, Order,
    ThumbnailSize,
};

fn gathered_file(bench: &Bench) -> PathBuf {
    bench.at("home/.config/nova-files/gathered.toml")
}

/// The next gathered folders `client` hears.
fn next(client: &Client) -> Arc<Gathered> {
    match client.next() {
        Event::Gathered(gathered) => gathered,
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
    let client = bench.client();
    client.send(Command::Gathered);
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
    let window = bench.client();
    let other = bench.client();
    window.send(Command::Gathered);
    other.send(Command::Gathered);
    assert_eq!(*next(&window), Gathered::default());
    assert_eq!(*next(&other), Gathered::default());

    window.send(Command::ChangeGathered(GatheredChange::Add {
        place: Gathering::Projects,
        path: skills.clone(),
        holds: None,
        at: None,
    }));
    let gathered = next(&window);
    assert_eq!(gathered.projects[0].holds, Holds::One);
    assert_eq!(next(&other), gathered);
    let text = fs::read_to_string(gathered_file(&bench)).unwrap();
    assert!(text.contains("\"~/Skills\""), "{text}");

    // Undo puts a removed folder back where it was.
    window.send(Command::ChangeGathered(GatheredChange::Add {
        place: Gathering::Projects,
        path: bench.at("home/Work"),
        holds: Some(Holds::Many),
        at: None,
    }));
    next(&window);
    window.send(Command::ChangeGathered(GatheredChange::Remove {
        place: Gathering::Projects,
        path: skills.clone(),
    }));
    assert_eq!(
        paths(&next(&window), Gathering::Projects),
        [bench.at("home/Work")]
    );
    window.send(Command::ChangeGathered(GatheredChange::Add {
        place: Gathering::Projects,
        path: skills.clone(),
        holds: Some(Holds::One),
        at: Some(0),
    }));
    assert_eq!(
        paths(&next(&window), Gathering::Projects),
        [skills, bench.at("home/Work")]
    );
}

#[test]
fn the_file_edited_by_hand_reaches_the_windows() {
    let bench = Bench::new("gathered-by-hand");
    let client = bench.client();
    client.send(Command::Gathered);
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

/// Lists `place` as listing `id` and returns the listing.
fn listed(client: &Client, id: u64, place: Gathering) -> Listing {
    client.send(Command::ListGathered {
        id: ListingId(id),
        place,
        order: Order::default(),
    });
    loop {
        match client.next() {
            Event::FolderListed(listing) if listing.id == ListingId(id) => return listing,
            Event::Gathered(_) | Event::GatheredChanged(_) => {}
            other => panic!("expected listing {id}, got {other:?}"),
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
    let client = bench.client();
    // Listing before asking for the folders reads them first.
    let listing = listed(&client, 1, Gathering::Projects);
    assert_eq!(names(&listing), ["bench", "nova"]);
    assert_eq!(
        listing.path_of(&listing.entries[1]),
        bench.at("home/Projects/nova")
    );
    assert_eq!(listing.path, PathBuf::new());

    // Its rows are served like a folder's.
    client.send(Command::Rows {
        id: ListingId(1),
        first: EntryId(0),
        count: 2,
        size: ThumbnailSize::Normal,
    });
    loop {
        if let Event::DetailsRead { id, .. } = client.next() {
            assert_eq!(id, ListingId(1));
            break;
        }
    }

    client.send(Command::Gathered);
    next(&client);
    fs::create_dir(bench.at("home/Projects/zed")).unwrap();
    loop {
        match client.next() {
            Event::GatheredChanged(Gathering::Projects) => break,
            Event::DetailsRead { .. } | Event::Thumbnail { .. } => {}
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
