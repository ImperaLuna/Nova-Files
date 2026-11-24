#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

mod common;

use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use common::Bench;
use nova_files_api::{
    Carried, Command, ConflictKind, Details, EntryId, EntryKind, Event, ListingId, Order, Outcome,
    SortKey, ThumbnailSize, Unreadable,
};
use nova_files_archives_api::{PASSWORD_NEEDED, Password};
use nova_files_open_api::{self as open_api, NotOpened};
use zip::DateTime;
use zip::write::SimpleFileOptions;

fn docs_zip(archive: &Path) {
    let mut zip = zip::ZipWriter::new(File::create(archive).unwrap());
    let options = SimpleFileOptions::default()
        .last_modified_time(DateTime::from_date_and_time(2020, 1, 2, 3, 4, 6).unwrap());
    zip.add_directory("docs/", options).unwrap();
    for (name, contents) in [
        ("docs/b.txt", "bb"),
        ("docs/a.txt", "aaaa"),
        ("top.txt", "t"),
    ] {
        zip.start_file(name, options).unwrap();
        zip.write_all(contents.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}

fn list(
    bench: &Bench,
    path: PathBuf,
    order: Order,
) -> Result<Vec<(String, EntryKind)>, Unreadable> {
    let id = ListingId(1);
    bench.send(Command::ListFolder {
        id,
        path,
        order,
        filter: String::new(),
    });
    loop {
        match bench.next() {
            Event::FolderListed(listing) if listing.id == id => {
                return Ok(listing
                    .entries
                    .iter()
                    .map(|entry| (entry.name.clone().into_string().unwrap(), entry.kind))
                    .collect());
            }
            Event::FolderUnreadable {
                id: got, reason, ..
            } if got == id => return Err(reason),
            _ => {}
        }
    }
}

fn details(bench: &Bench, count: u32) -> Vec<Details> {
    let id = ListingId(1);
    bench.send(Command::Rows {
        id,
        first: EntryId(0),
        count,
        size: ThumbnailSize::Normal,
        peeks: false,
    });
    loop {
        if let Event::DetailsRead {
            id: got, details, ..
        } = bench.next()
            && got == id
        {
            return details.to_vec();
        }
    }
}

#[test]
fn a_folder_inside_lists_folders_first_then_by_name() {
    let bench = Bench::new("inside-list");
    docs_zip(&bench.at("a.zip"));
    let top = list(&bench, bench.at("a.zip"), Order::default()).unwrap();
    assert_eq!(
        top,
        [
            ("docs".to_owned(), EntryKind::Folder),
            ("top.txt".to_owned(), EntryKind::File)
        ]
    );
    let docs = list(&bench, bench.at("a.zip/docs"), Order::default()).unwrap();
    assert_eq!(
        docs,
        [
            ("a.txt".to_owned(), EntryKind::File),
            ("b.txt".to_owned(), EntryKind::File)
        ]
    );
}

#[test]
fn a_folder_inside_sorts_by_size_and_tells_its_details() {
    let bench = Bench::new("inside-size");
    docs_zip(&bench.at("a.zip"));
    let order = Order {
        key: SortKey::Size,
        descending: false,
    };
    let docs = list(&bench, bench.at("a.zip/docs"), order).unwrap();
    assert_eq!(docs[0].0, "b.txt");
    let read = details(&bench, 2);
    assert_eq!(read[0].size, Some(2));
    assert_eq!(read[1].size, Some(4));
    let modified = read[0].modified.unwrap();
    let seconds = modified.duration_since(UNIX_EPOCH).unwrap();
    assert!(seconds > Duration::from_secs(1_577_000_000));
}

#[test]
fn a_path_not_in_the_archive_is_missing() {
    let bench = Bench::new("inside-missing");
    docs_zip(&bench.at("a.zip"));
    let listed = list(&bench, bench.at("a.zip/nothing"), Order::default());
    assert_eq!(listed, Err(Unreadable::Missing));
    let file = list(&bench, bench.at("a.zip/top.txt"), Order::default());
    assert_eq!(file, Err(Unreadable::NotAFolder));
}

#[test]
fn copying_out_takes_only_the_picked_entries() {
    let bench = Bench::new("inside-copy");
    docs_zip(&bench.at("a.zip"));
    let job = bench.start(nova_files_transfer_api::copy(
        vec![bench.at("a.zip/docs/a.txt"), bench.at("a.zip/top.txt")],
        bench.at("into"),
    ));
    assert_eq!(bench.finished(job).0, Outcome::Done);
    assert_eq!(bench.landed(), ["a.txt", "top.txt"]);
    assert_eq!(fs::read(bench.at("into/a.txt")).unwrap(), b"aaaa");
}

#[test]
fn copying_out_a_folder_takes_all_inside_it() {
    let bench = Bench::new("inside-copy-folder");
    docs_zip(&bench.at("a.zip"));
    let job = bench.start(nova_files_transfer_api::copy(
        vec![bench.at("a.zip/docs")],
        bench.at("into"),
    ));
    assert_eq!(bench.finished(job).0, Outcome::Done);
    assert_eq!(bench.landed(), ["docs"]);
    assert_eq!(fs::read(bench.at("into/docs/b.txt")).unwrap(), b"bb");
}

fn opened(bench: &Bench, path: &Path) -> Result<(), NotOpened> {
    bench.send(open_api::Command::OpenFile {
        path: path.to_path_buf(),
        activation: None,
    });
    loop {
        match bench.next() {
            open_api::Event::Opened { path: got } if got == path => return Ok(()),
            open_api::Event::NotOpened { path: got, reason } if got == path => return Err(reason),
            _ => {}
        }
    }
}

#[test]
fn a_file_inside_is_taken_out_to_open_it() {
    let bench = Bench::new("inside-open");
    docs_zip(&bench.at("a.zip"));
    assert_ne!(
        opened(&bench, &bench.at("a.zip/docs/a.txt")),
        Err(NotOpened::Failed)
    );
    assert_eq!(
        opened(&bench, &bench.at("a.zip/docs/none.txt")),
        Err(NotOpened::Failed)
    );
}

#[test]
fn copying_out_a_locked_entry_asks_for_its_password() {
    let bench = Bench::new("inside-locked");
    let mut zip = zip::ZipWriter::new(File::create(bench.at("a.zip")).unwrap());
    let locked = SimpleFileOptions::default().with_aes_encryption(zip::AesMode::Aes256, "pw");
    zip.start_file("secret.txt", locked).unwrap();
    zip.write_all(b"hidden").unwrap();
    zip.finish().unwrap();
    let job = bench.start(nova_files_transfer_api::copy(
        vec![bench.at("a.zip/secret.txt")],
        bench.at("into"),
    ));
    let needed = bench.held(job);
    assert_eq!(needed.kind, ConflictKind::Asked(&PASSWORD_NEEDED));
    bench.send(Command::Reply {
        job,
        conflict: needed,
        reply: Carried::new(Password("pw".to_owned())),
    });
    assert_eq!(bench.finished(job).0, Outcome::Done);
    assert_eq!(fs::read(bench.at("into/secret.txt")).unwrap(), b"hidden");
}
