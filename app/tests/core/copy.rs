use crate::common;
use crate::files;

use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, SystemTime};

use nova_files_api::uri::file_uri;
use nova_files_api::{Command, Event, Failure, JobId, Operation, Outcome, Progress};
use nova_files_clipboard_api::{FileClipboard, Offer, URI_LIST};

use common::{Bench, Client};
use files::{hidden, tree};

fn copy(sources: Vec<PathBuf>, into: &Path) -> Operation {
    Operation::Copy {
        sources,
        into: into.to_owned(),
    }
}

const MANY: usize = 400;
const FILE_BYTES: usize = 16 * 1024;
const FOLDER_MODE: u32 = 0o750;
const HOLD: Duration = Duration::from_millis(300);
const JOURNAL_WAIT: Duration = Duration::from_millis(50);
const JOURNAL_WAITS: u32 = 200;

fn small_tree(bench: &Bench) -> PathBuf {
    let source = bench.at("src");
    fs::create_dir_all(source.join("sub/deeper")).unwrap();
    fs::create_dir(source.join("empty")).unwrap();
    fs::write(source.join("a.txt"), "alpha").unwrap();
    fs::write(source.join("sub/deeper/b"), "beta").unwrap();
    fs::write(source.join(OsStr::from_bytes(b"caf\xe9")), "odd").unwrap();
    symlink("a.txt", source.join("link")).unwrap();
    symlink("/nowhere", source.join("broken")).unwrap();
    source
}

fn big_tree(bench: &Bench) -> PathBuf {
    let source = bench.at("big");
    fs::create_dir(&source).unwrap();
    for index in 0..MANY {
        fs::write(source.join(index.to_string()), vec![b'x'; FILE_BYTES]).unwrap();
    }
    source
}

fn pause(client: &Client, job: JobId) {
    client.send(Command::PauseJob(job));
    loop {
        match client.next() {
            Event::JobPaused(paused) if paused == job => return,
            Event::JobProgress { .. } => {}
            other => panic!("expected the job to pause, got {other:?}"),
        }
    }
}

fn next_besides_progress(client: &Client) -> Event {
    loop {
        match client.next() {
            Event::JobProgress { .. } => {}
            other => return other,
        }
    }
}

fn journal_begun(client: &Client, path: &Path) {
    let begun = "nova-files-job 1\ncopy\ninto ";
    for _ in 0..JOURNAL_WAITS {
        if fs::read_to_string(path).is_ok_and(|written| written.starts_with(begun)) {
            return;
        }
        assert_eq!(
            client.events.recv_timeout(JOURNAL_WAIT).err(),
            Some(RecvTimeoutError::Timeout),
            "nothing is heard while paused"
        );
    }
    panic!("the paused job never began its journal");
}

fn settle(client: &Client) {
    while let Ok(event) = client.events.recv_timeout(HOLD) {
        assert!(
            matches!(event, Event::JobProgress { .. }),
            "unexpected {event:?}"
        );
    }
}

#[test]
fn copies_a_tree_identically() {
    let bench = Bench::new("copy-tree");
    let source = small_tree(&bench);
    let sub = source.join("sub");
    fs::set_permissions(&sub, fs::Permissions::from_mode(FOLDER_MODE)).unwrap();
    let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    fs::File::open(&sub)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    let into = bench.at("into");
    fs::create_dir(&into).unwrap();
    let client = bench.client();

    let job = client.start(copy(vec![source.clone(), bench.at("src/a.txt")], &into));
    let mut last = Progress::default();
    let (outcome, changed) = loop {
        match client.next() {
            Event::JobProgress { progress, .. } => last = progress,
            Event::JobFinished {
                outcome, changed, ..
            } => break (outcome, changed),
            other => panic!("unexpected {other:?}"),
        }
    };

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(changed, vec![into.clone()]);
    assert_eq!(tree(&into.join("src")), tree(&source));
    let copied_sub = fs::metadata(into.join("src/sub")).unwrap();
    assert_eq!(copied_sub.permissions().mode() & 0o7777, FOLDER_MODE);
    assert_eq!(copied_sub.modified().unwrap(), modified);
    assert_eq!(fs::read_to_string(into.join("a.txt")).unwrap(), "alpha");
    assert_eq!(
        last,
        Progress {
            done_bytes: 17,
            total_bytes: 17,
            done_items: 10,
            total_items: 10,
        }
    );
    assert!(!journal(&bench, job).exists());
}

#[test]
fn a_copy_into_its_own_folder_is_refused() {
    let bench = Bench::new("copy-itself");
    let source = small_tree(&bench);
    let client = bench.client();

    let (_, outcome, changed) = client.run(copy(vec![source.clone()], &source.join("sub")));

    assert_eq!(
        outcome,
        Outcome::Failed {
            path: source.clone(),
            reason: Failure::IntoItself
        }
    );
    assert_eq!(changed, Vec::<PathBuf>::new());
    assert!(!source.join("sub/src").exists());
}

#[test]
fn a_paused_copy_holds_until_resumed() {
    let bench = Bench::new("copy-pause");
    let source = big_tree(&bench);
    let into = bench.at("into");
    fs::create_dir(&into).unwrap();
    let client = bench.client();

    let job = client.start(copy(vec![source.clone()], &into));
    pause(&client, job);
    settle(&client);
    let held = tree(&into);
    assert_eq!(
        client.events.recv_timeout(HOLD).err(),
        Some(RecvTimeoutError::Timeout)
    );
    assert_eq!(tree(&into), held, "nothing changes while paused");
    journal_begun(&client, &journal(&bench, job));

    client.send(Command::ResumeJob(job));
    assert_eq!(next_besides_progress(&client), Event::JobResumed(job));
    let (outcome, _) = client.ended(job);

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(tree(&into.join("big")), tree(&source));
    assert!(!journal(&bench, job).exists());
}

#[test]
fn a_cancelled_copy_leaves_only_whole_files() {
    let bench = Bench::new("copy-cancel");
    let source = big_tree(&bench);
    let into = bench.at("into");
    fs::create_dir(&into).unwrap();
    let client = bench.client();

    let job = client.start(copy(vec![source], &into));
    pause(&client, job);
    client.send(Command::CancelJob(job));
    let (outcome, changed) = client.ended(job);

    assert_eq!(outcome, Outcome::Cancelled);
    let made = into.join("big").exists();
    assert_eq!(changed, if made { vec![into.clone()] } else { Vec::new() });
    let copied = if made {
        tree(&into.join("big"))
    } else {
        Vec::new()
    };
    assert!(copied.len() < MANY);
    let whole = format!("file {:?}", vec![b'x'; FILE_BYTES]);
    assert!(copied.iter().all(|(_, what)| *what == whole));
    assert_eq!(hidden(&bench.root), Vec::<PathBuf>::new());
    assert!(!journal(&bench, job).exists());
}

#[test]
fn pausing_or_resuming_twice_tells_once() {
    let bench = Bench::new("copy-twice");
    let source = big_tree(&bench);
    let client = bench.client();

    let job = client.start(copy(vec![source], &bench.at("home")));
    pause(&client, job);
    client.send(Command::PauseJob(job));
    client.send(Command::ResumeJob(job));
    assert_eq!(next_besides_progress(&client), Event::JobResumed(job));
    client.send(Command::ResumeJob(job));
    client.send(Command::CancelJob(job));

    let (outcome, _) = client.ended(job);
    assert_eq!(outcome, Outcome::Cancelled);
    client.send(Command::PauseJob(job));
    client.send(Command::Ping);
    assert_eq!(client.next(), Event::Pong);
}

#[test]
fn a_paused_job_keeps_the_core_up_after_its_window_closes() {
    let idle_exit = Duration::from_millis(200);
    let bench = Bench::with("copy-alive", idle_exit, |_| {});
    let client = bench.client();
    let source = big_tree(&bench);
    let operation = copy(vec![source], &bench.at("home"));
    let job = client.start(operation.clone());
    pause(&client, job);

    drop(client);
    wait(idle_exit * 3);
    assert!(!bench.core.thread.is_finished());

    let client = bench.client();
    client.send(Command::WatchJobs);
    let Event::JobRunning {
        job: heard,
        operation: doing,
        progress,
        paused,
        conflict,
    } = client.next()
    else {
        panic!("the held job comes first");
    };
    assert_eq!(
        (heard, doing, paused, conflict),
        (job, operation, true, None)
    );
    assert!([0, MANY as u64 + 1].contains(&progress.total_items));
    client.send(Command::CancelJob(job));
    client.ended(job);
    drop(client);
    wait(idle_exit * 3);
    assert!(bench.core.thread.is_finished(), "the core stops once idle");
}

fn wait(time: Duration) {
    let (_never_sent, quiet) = mpsc::channel::<()>();
    assert_eq!(quiet.recv_timeout(time), Err(RecvTimeoutError::Timeout));
}

fn journal(bench: &Bench, job: JobId) -> PathBuf {
    bench
        .at("home/.local/state/nova-files/jobs")
        .join(job.0.to_string())
}

#[test]
fn a_copy_pastes_as_a_copy_and_leaves_the_files() {
    let bench = Bench::new("clipboard-copy");
    fs::create_dir(bench.at("from")).unwrap();
    fs::write(bench.at("from/a"), b"a").unwrap();
    fs::create_dir(bench.at("into")).unwrap();
    let list = format!("{}\r\n", file_uri(&bench.at("from/a")));
    let files = FileClipboard::from_offers(&[Offer::new(URI_LIST, list)]).unwrap();

    let (_, outcome, _) = bench
        .client()
        .run(files.paste_into(bench.at("into")).unwrap());

    assert_eq!(outcome, Outcome::Done);
    assert_eq!(fs::read(bench.at("from/a")).unwrap(), b"a");
    assert_eq!(fs::read(bench.at("into/a")).unwrap(), b"a");
}
