use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use nova_files_api::{DiskSpace, Event, EventSink, MAX_DISK_SPACE_PATHS, PathSpace};
use rustix::fs::statvfs;

use crate::timed;

const THREAD_NAME: &str = "nova-files-disk-space";
const READ_THREAD: &str = "nova-files-statvfs";
pub const READ_WAIT: Duration = Duration::from_secs(2);

pub fn answer(events: &Arc<dyn EventSink>, mut paths: Vec<PathBuf>) {
    paths.truncate(MAX_DISK_SPACE_PATHS);
    let sink = Arc::clone(events);
    let asked = paths.clone();
    let spawned = thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            let spaces = paths
                .into_iter()
                .map(|path| {
                    let asked = path.clone();
                    PathSpace {
                        space: timed::within(READ_WAIT, READ_THREAD, move || read(&asked))
                            .flatten(),
                        path,
                    }
                })
                .collect();
            sink.deliver(Event::DiskSpace(spaces)).ok();
        });
    if spawned.is_err() {
        let unknown = asked
            .into_iter()
            .map(|path| PathSpace { path, space: None })
            .collect();
        events.deliver(Event::DiskSpace(unknown)).ok();
    }
}

fn read(path: &Path) -> Option<DiskSpace> {
    if !path.is_absolute() {
        return None;
    }
    let drive = statvfs(path).ok()?;
    let block = drive.f_frsize;
    Some(DiskSpace {
        total: drive.f_blocks.saturating_mul(block),
        free: drive.f_bavail.saturating_mul(block),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_or_missing_path_has_no_space() {
        assert_eq!(read(Path::new("relative")), None);
        assert_eq!(read(Path::new("/no/such/folder/anywhere")), None);
        let root = read(Path::new("/")).unwrap_or_default();
        assert!(root.total > 0);
        assert!(root.free <= root.total);
    }
}
