//! Folders' views through the core: a client that asks hears every view
//! set, and the next core reads them from the state folder.

#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use nova_files_api::{Command, Event, FolderView, FolderViews, IconSize, ViewMode};
use nova_files_core::{Home, IDLE_EXIT, RunningCore};

/// How long a test waits for the core before it counts as a failure.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);

const ICONS: FolderView = FolderView {
    mode: ViewMode::Icons,
    icons: IconSize::Large,
};

struct TempHome(PathBuf);

impl TempHome {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "nova-files-folder-views-{name}-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&path).ok();
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn core(&self) -> RunningCore {
        let home = Home {
            folder: self.0.clone(),
            config: self.0.join("config"),
            state: self.0.join("state"),
            data: self.0.join("share"),
            drive_trashes: false,
        };
        nova_files_core::spawn_with(IDLE_EXIT, Some(home), None, None).unwrap()
    }
}

impl TempHome {
    /// Waits until the state file names `folder`.
    fn wait_for_save(&self, folder: &Path) {
        let file = self.0.join("state/nova-files/folder-views");
        let name = folder.as_os_str().as_encoded_bytes();
        let start = Instant::now();
        while !fs::read(&file).is_ok_and(|bytes| bytes.windows(name.len()).any(|at| at == name)) {
            assert!(
                start.elapsed() < ANSWER_TIMEOUT,
                "{} never named {}",
                file.display(),
                folder.display()
            );
            std::thread::yield_now();
        }
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// The next folder views `events` gets.
fn next_views(events: &Receiver<Event>) -> FolderViews {
    loop {
        if let Event::FolderViews(views) = events.recv_timeout(ANSWER_TIMEOUT).unwrap() {
            return views;
        }
    }
}

#[test]
fn a_view_set_reaches_the_client_and_the_next_core() {
    let home = TempHome::new("set");
    let folder = Path::new("/home/me/Pictures").to_owned();
    {
        let core = home.core();
        let (sink, events) = mpsc::channel();
        let client = core.connector.connect(sink).unwrap();
        client.send(Command::FolderViews).unwrap();
        assert!(next_views(&events).is_empty());
        client
            .send(Command::SetFolderView {
                folder: folder.clone(),
                view: ICONS,
            })
            .unwrap();
        assert_eq!(*next_views(&events), [(folder.clone(), ICONS)]);
    }
    home.wait_for_save(&folder);
    let next = home.core();
    let (sink, events) = mpsc::channel();
    let client = next.connector.connect(sink).unwrap();
    client.send(Command::FolderViews).unwrap();
    assert_eq!(*next_views(&events), [(folder, ICONS)]);
}
