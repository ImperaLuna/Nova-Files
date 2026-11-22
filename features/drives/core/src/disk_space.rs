use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use nova_files_drives_api::{DiskSpace, Event, MAX_DISK_SPACE_PATHS, PathSpace};
use nova_files_messages::EventSink;
use rustix::fs::statvfs;

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
            let spaces = spaces(paths, READ_WAIT, read);
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

fn spaces(
    paths: Vec<PathBuf>,
    wait: Duration,
    read: impl Fn(&Path) -> Option<DiskSpace> + Send + Sync + 'static,
) -> Vec<PathSpace> {
    let read = Arc::new(read);
    let (done, answers) = mpsc::channel();
    for (index, path) in paths.iter().enumerate() {
        let (done, read, path) = (done.clone(), Arc::clone(&read), path.clone());
        thread::Builder::new()
            .name(READ_THREAD.to_owned())
            .spawn(move || done.send((index, read(&path))).ok())
            .ok();
    }
    drop(done);
    let deadline = Instant::now() + wait;
    let mut found: Vec<Option<DiskSpace>> = paths.iter().map(|_| None).collect();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Ok((index, space)) = answers.recv_timeout(left) else {
            break;
        };
        found[index] = space;
    }
    paths
        .into_iter()
        .zip(found)
        .map(|(path, space)| PathSpace { path, space })
        .collect()
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
    fn stuck_drives_answer_nothing_and_hold_back_no_other_drive() {
        const WAIT: Duration = Duration::from_millis(200);
        let (_hold, stuck) = mpsc::channel::<()>();
        let stuck = std::sync::Mutex::new(stuck);
        let paths = ["/stuck/a", "/stuck/b", "/stuck/c", "/"].map(PathBuf::from);

        let spaces = spaces(paths.to_vec(), WAIT, move |path| {
            if path.starts_with("/stuck") {
                stuck.lock().ok()?.recv().ok()?;
            }
            Some(DiskSpace { total: 1, free: 1 })
        });

        let found: Vec<_> = spaces
            .iter()
            .map(|space| (space.path.as_path(), space.space))
            .collect();
        assert_eq!(
            found,
            [
                (Path::new("/stuck/a"), None),
                (Path::new("/stuck/b"), None),
                (Path::new("/stuck/c"), None),
                (Path::new("/"), Some(DiskSpace { total: 1, free: 1 })),
            ]
        );
    }

    #[test]
    fn a_relative_or_missing_path_has_no_space() {
        assert_eq!(read(Path::new("relative")), None);
        assert_eq!(read(Path::new("/no/such/folder/anywhere")), None);
        let root = read(Path::new("/")).unwrap_or_default();
        assert!(root.total > 0);
        assert!(root.free <= root.total);
    }
}
