//! Answers [`Command::DiskSpace`]: `statvfs` on each path asked, in order.
//!
//! Each request reads on a thread of its own: a network drive that stopped
//! answering holds up only that answer, never the core or other clients.
//!
//! [`Command::DiskSpace`]: nova_files_api::Command::DiskSpace

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use nova_files_api::{DiskSpace, Event, EventSink, MAX_DISK_SPACE_PATHS, PathSpace};
use rustix::fs::statvfs;

const THREAD_NAME: &str = "nova-files-disk-space";

/// Reads the space of the drives holding `paths` on a thread of its own
/// and hands the answer to `events`. When no thread can start, every path
/// answers `None`; the client can ask again.
pub fn answer(events: &Arc<dyn EventSink>, mut paths: Vec<PathBuf>) {
    paths.truncate(MAX_DISK_SPACE_PATHS);
    let sink = Arc::clone(events);
    let asked = paths.clone();
    let spawned = thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            let spaces = paths
                .into_iter()
                .map(|path| PathSpace {
                    space: read(&path),
                    path,
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

/// The space of the drive holding `path`, or `None` when it can't be read
/// or `path` isn't absolute.
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
