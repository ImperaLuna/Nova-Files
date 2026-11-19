#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

#[expect(dead_code, reason = "the job helpers; properties need only the bench")]
mod common;
mod feature;

use std::fs;

use nova_files_api::Unreadable;
use nova_files_properties_api::{Command, Event, PathKind, PropertiesId};

use common::Bench;
use feature::Feature;

type Client = Feature<Event>;

const FILE_BYTES: u64 = 42;

#[test]
fn a_client_hears_a_files_properties() {
    let bench = Bench::new("properties-file");
    let path = bench.at("notes.txt");
    fs::write(&path, [0; 42]).unwrap();
    let client = Client::of(&bench);
    client.send(Command::Properties {
        id: PropertiesId(1),
        path: path.clone(),
    });
    match client.next() {
        Event::Properties { id, properties } => {
            assert_eq!(id, PropertiesId(1));
            assert_eq!(properties.path, path);
            assert_eq!(properties.kind, PathKind::File);
            assert_eq!(properties.bytes, FILE_BYTES);
        }
        other => panic!("expected the properties, got {other:?}"),
    }
}

#[test]
fn a_missing_path_is_said_to_be_missing() {
    let bench = Bench::new("properties-missing");
    let client = Client::of(&bench);
    client.send(Command::Properties {
        id: PropertiesId(2),
        path: bench.at("gone"),
    });
    match client.next() {
        Event::PropertiesUnreadable { id, reason, .. } => {
            assert_eq!(id, PropertiesId(2));
            assert_eq!(reason, Unreadable::Missing);
        }
        other => panic!("expected the path to be missing, got {other:?}"),
    }
}
