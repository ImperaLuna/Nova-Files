use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use nova_files_api::{EventSink, JobTag, Operation};
use nova_files_disk::timed;
use nova_files_jobs::{JobKind, Jobs, lock};
use nova_files_transfer_api::{DropAskId, DropKind, Event, copy, move_into};

const PROBE_WAIT: Duration = Duration::from_secs(2);

const PROBE_THREAD: &str = "nova-files-drop-probe";

const ASK_THREAD: &str = "nova-files-drop-ask";

const DROP_THREAD: &str = "nova-files-drop";

pub fn dropped(sources: Vec<PathBuf>, into: PathBuf) -> Operation {
    match kind(&sources, &into) {
        DropKind::Move => move_into(sources, into),
        DropKind::Copy => copy(sources, into),
    }
}

pub fn start<K: JobKind<Op = Operation>>(
    jobs: &Arc<Jobs<K>>,
    events: &Arc<dyn EventSink>,
    tag: JobTag,
    sources: Vec<PathBuf>,
    into: PathBuf,
) {
    let thread_jobs = Arc::clone(jobs);
    let asked = Arc::new(Mutex::new(Some((sources, into))));
    let taken = Arc::clone(&asked);
    let thread_events = Arc::clone(events);
    let spawned = thread::Builder::new()
        .name(DROP_THREAD.to_owned())
        .spawn(move || {
            let asked = lock(&taken).take();
            if let Some((sources, into)) = asked {
                let operation = dropped(sources, into);
                thread_jobs
                    .start(thread_events.as_ref(), tag, operation)
                    .ok();
            }
        });
    let unasked = spawned.is_err().then(|| lock(&asked).take()).flatten();
    if let Some((sources, into)) = unasked {
        jobs.start(events.as_ref(), tag, copy(sources, into)).ok();
    }
}

pub fn answer(events: &Arc<dyn EventSink>, id: DropAskId, sources: Vec<PathBuf>, into: PathBuf) {
    let sink = Arc::clone(events);
    let spawned = thread::Builder::new()
        .name(ASK_THREAD.to_owned())
        .spawn(move || {
            let kind = kind(&sources, &into);
            sink.deliver(Event::DropWould { id, kind }).ok();
        });
    if spawned.is_err() {
        let kind = DropKind::Copy;
        events.deliver(Event::DropWould { id, kind }).ok();
    }
}

fn kind(sources: &[PathBuf], into: &Path) -> DropKind {
    let (asked, folder) = (sources.to_vec(), into.to_path_buf());
    let one_drive = timed::within(PROBE_WAIT, PROBE_THREAD, move || {
        on_one_drive(&asked, &folder)
    });
    if one_drive == Some(true) {
        DropKind::Move
    } else {
        DropKind::Copy
    }
}

fn on_one_drive(sources: &[PathBuf], into: &Path) -> bool {
    let Ok(drive) = fs::metadata(into).map(|folder| folder.dev()) else {
        return false;
    };
    sources.iter().all(|source| {
        source
            .symlink_metadata()
            .is_ok_and(|source| source.dev() == drive)
    })
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;
    use nova_files_test_folder::TestFolder;

    const ANOTHER_DRIVE: &str = "/proc";

    #[test]
    fn files_on_the_folders_drive_move() {
        let root = TestFolder::new("drop-move");
        let file = root.file("a", "a");
        let into = root.path().join("in");
        fs::create_dir(&into).unwrap();
        assert_eq!(
            dropped(vec![file.clone()], into.clone()),
            move_into(vec![file], into)
        );
    }

    #[test]
    fn files_from_another_drive_are_copied() {
        let root = TestFolder::new("drop-copy");
        let source = PathBuf::from(ANOTHER_DRIVE).join("self");
        let into = root.path().to_path_buf();
        assert_eq!(
            dropped(vec![source.clone()], into.clone()),
            copy(vec![source], into)
        );
    }

    #[test]
    fn a_source_or_folder_that_isnt_there_copies() {
        let root = TestFolder::new("drop-gone");
        let file = root.file("a", "a");
        let gone = root.path().join("gone");
        assert!(!on_one_drive(&[file.clone(), gone.clone()], root.path()));
        assert!(!on_one_drive(&[file], &gone));
    }

    #[test]
    fn a_link_counts_where_it_is_and_the_folder_where_it_points() {
        let root = TestFolder::new("drop-links");
        let link = root.path().join("link");
        symlink(ANOTHER_DRIVE, &link).unwrap();
        assert!(on_one_drive(std::slice::from_ref(&link), root.path()));
        let file = root.file("a", "a");
        assert!(!on_one_drive(&[file], &link));
    }
}
