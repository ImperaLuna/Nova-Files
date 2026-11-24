use crate::common;

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::process::Command as Process;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};

use nova_files_api::{Connection, EntryKind, Fit, Unreadable};
use nova_files_preview_api::{
    Command, Event, Mark, PREVIEW_ENTRIES, PREVIEW_TEXT_BYTES, Preview, PreviewContent, PreviewId,
};

use common::{ANSWER_TIMEOUT, Bench};

const FIT: Fit = Fit {
    width: 800,
    height: 600,
};

struct Client {
    connection: Connection,
    events: Receiver<Event>,
}

fn client(bench: &Bench) -> Client {
    let (sink, events) = mpsc::channel();
    let connection = bench.core.connector.connect(sink).unwrap();
    Client { connection, events }
}

fn preview(client: &Client, id: u64, path: PathBuf) -> Result<Arc<Preview>, Unreadable> {
    let command = Command::Preview {
        id: PreviewId(id),
        path,
        fit: FIT,
    };
    client.connection.send(command).unwrap();
    match client.events.recv_timeout(ANSWER_TIMEOUT).unwrap() {
        Event::Previewed {
            id: answered,
            preview,
        } => {
            assert_eq!(answered, PreviewId(id));
            Ok(preview)
        }
        Event::Unreadable {
            id: answered,
            reason,
            ..
        } => {
            assert_eq!(answered, PreviewId(id));
            Err(reason)
        }
    }
}

#[test]
fn a_text_file_shows_its_text_and_size() {
    let bench = Bench::new("preview-text");
    let path = bench.at("notes.md");
    let notes = "# Notes\n\n- one\n";
    fs::write(&path, notes).unwrap();
    let shown = preview(&client(&bench), 1, path.clone()).unwrap();
    assert_eq!(shown.path, path);
    assert_eq!(shown.bytes, u64::try_from(notes.len()).unwrap());
    assert_eq!(
        shown.content,
        PreviewContent::Text {
            text: notes.into(),
            cut: false
        }
    );
}

#[test]
fn a_source_file_shows_its_text_with_marks() {
    let bench = Bench::new("preview-code");
    let path = bench.at("main.rs");
    let code = "fn main() {} // start\n";
    fs::write(&path, code).unwrap();
    let shown = preview(&client(&bench), 1, path).unwrap();
    match &shown.content {
        PreviewContent::Code { text, cut, runs } => {
            assert_eq!(&**text, code);
            assert!(!cut);
            let covered: u32 = runs.iter().map(|run| run.bytes).sum();
            assert_eq!(covered as usize, code.len());
            assert!(runs.iter().any(|run| run.mark == Mark::Comment));
        }
        other => panic!("expected code, got {other:?}"),
    }
}

#[test]
fn a_long_text_file_shows_its_start() {
    let bench = Bench::new("preview-long");
    let path = bench.at("log.txt");
    fs::write(&path, "a".repeat(PREVIEW_TEXT_BYTES * 2)).unwrap();
    let shown = preview(&client(&bench), 1, path).unwrap();
    match &shown.content {
        PreviewContent::Text { text, cut } => {
            assert_eq!(text.len(), PREVIEW_TEXT_BYTES);
            assert!(cut);
        }
        other => panic!("expected text, got {other:?}"),
    }
}

#[test]
fn binary_and_empty_files_show_their_icon() {
    let bench = Bench::new("preview-binary");
    fs::write(bench.at("data.bin"), [1, 2, 0, 3]).unwrap();
    fs::write(bench.at("empty"), "").unwrap();
    let client = client(&bench);
    let binary = preview(&client, 1, bench.at("data.bin")).unwrap();
    assert_eq!(binary.content, PreviewContent::Icon);
    assert_eq!(binary.bytes, 4);
    let empty = preview(&client, 2, bench.at("empty")).unwrap();
    assert_eq!(empty.content, PreviewContent::Icon);
}

#[test]
fn a_pipe_shows_its_icon_without_waiting_for_a_writer() {
    let bench = Bench::new("preview-pipe");
    let pipe = bench.at("pipe");
    let made = Process::new("mkfifo").arg(&pipe).status().unwrap();
    assert!(made.success());
    let shown = preview(&client(&bench), 1, pipe).unwrap();
    assert_eq!(shown.content, PreviewContent::Icon);
}

#[test]
fn a_folder_shows_its_first_entries_and_how_many_it_holds() {
    let bench = Bench::new("preview-folder");
    let folder = bench.at("photos");
    fs::create_dir_all(folder.join("b-folder")).unwrap();
    let files = PREVIEW_ENTRIES + 3;
    for n in 0..files {
        fs::write(folder.join(format!("a-{n:02}.txt")), "x").unwrap();
    }
    let shown = preview(&client(&bench), 1, folder).unwrap();
    assert_eq!(shown.bytes, 0);
    assert_eq!(shown.kind_name, None);
    match &shown.content {
        PreviewContent::Folder { first, count } => {
            assert_eq!(*count, files + 1);
            assert_eq!(first.len(), PREVIEW_ENTRIES);
            assert_eq!(first[0].name, "b-folder");
            assert_eq!(first[0].kind, EntryKind::Folder);
            assert_eq!(first[1].name, "a-00.txt");
        }
        other => panic!("expected a folder, got {other:?}"),
    }
}

#[test]
fn a_link_shows_what_it_points_to() {
    let bench = Bench::new("preview-link");
    fs::write(bench.at("target.txt"), "hello").unwrap();
    symlink(bench.at("target.txt"), bench.at("link")).unwrap();
    let shown = preview(&client(&bench), 1, bench.at("link")).unwrap();
    assert_eq!(shown.bytes, 5);
    assert!(matches!(shown.content, PreviewContent::Text { .. }));
}

#[test]
fn a_missing_path_is_said_to_be_missing() {
    let bench = Bench::new("preview-missing");
    let client = client(&bench);
    assert_eq!(
        preview(&client, 7, bench.at("gone")).unwrap_err(),
        Unreadable::Missing
    );
}
