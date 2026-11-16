//! What dragging files onto a folder does, as other file managers do it:
//! files on the folder's own drive move, files from another drive are
//! copied, so a drag never takes files off the drive they were on.
//!
//! A drive here is a filesystem: two paths are on one drive when the
//! system gives them the same device. Each source is looked at itself,
//! never through a link (a drive mounted on a folder dragged counts as
//! that drive), and the folder through any link to it. A source or a
//! folder that can't be looked at makes the drop a copy, which leaves
//! every source where it was, and so does a drive that doesn't answer
//! within [`PROBE_WAIT`] (a network share whose server went away): the
//! drop still starts a job the user can see and cancel. This looks at the
//! disk: the caller runs it off the core's loop (see `Jobs::start_drop`).
//!
//! A drag asks the same before it lands ([`answer`]), so it can say "Move
//! to" or "Copy to"; that answer comes from the same rule on a thread of
//! its own, and a drop made after it picks again.

use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use nova_files_api::{DropAskId, DropKind, Event, EventSink, Operation};

use crate::timed;

/// How long the drives are looked at before the drop is made a copy.
const PROBE_WAIT: Duration = Duration::from_secs(2);

const PROBE_THREAD: &str = "nova-files-drop-probe";

const ASK_THREAD: &str = "nova-files-drop-ask";

/// The job dropping `sources` onto the folder `into` runs.
pub fn dropped(sources: Vec<PathBuf>, into: PathBuf) -> Operation {
    match kind(&sources, &into) {
        DropKind::Move => Operation::Move { sources, into },
        DropKind::Copy => Operation::Copy { sources, into },
    }
}

/// Answers ask `id`, what dropping `sources` onto `into` would do, to
/// `events`, from a thread of its own. When no thread can start, it
/// answers a copy, as such a drop does.
pub fn answer(events: &Arc<dyn EventSink>, id: DropAskId, sources: Vec<PathBuf>, into: PathBuf) {
    let sink = Arc::clone(events);
    let spawned = thread::Builder::new()
        .name(ASK_THREAD.to_owned())
        .spawn(move || {
            let kind = kind(&sources, &into);
            // A client gone needs no answer.
            sink.deliver(Event::DropWould { id, kind }).ok();
        });
    if spawned.is_err() {
        let kind = DropKind::Copy;
        events.deliver(Event::DropWould { id, kind }).ok();
    }
}

/// Whether dropping `sources` onto the folder `into` moves or copies.
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

/// Whether every one of `sources` is on the drive of the folder `into`.
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
    use crate::ops::test_folder::TestFolder;

    /// `/proc` is never on the drive of a temp folder.
    const ANOTHER_DRIVE: &str = "/proc";

    #[test]
    fn files_on_the_folders_drive_move() {
        let root = TestFolder::new("drop-move");
        let file = root.file("a", "a");
        let into = root.path().join("in");
        fs::create_dir(&into).unwrap();
        assert_eq!(
            dropped(vec![file.clone()], into.clone()),
            Operation::Move {
                sources: vec![file],
                into
            }
        );
    }

    #[test]
    fn files_from_another_drive_are_copied() {
        let root = TestFolder::new("drop-copy");
        let source = PathBuf::from(ANOTHER_DRIVE).join("self");
        let into = root.path().to_path_buf();
        assert_eq!(
            dropped(vec![source.clone()], into.clone()),
            Operation::Copy {
                sources: vec![source],
                into
            }
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
