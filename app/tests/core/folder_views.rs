use std::fs;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use crate::common::Core;
use nova_files::{Home, IDLE_EXIT};
use nova_files_folders_api::{Command, Event, FolderView, FolderViews, IconSize, ViewMode};
use nova_files_test_folder::TestFolder;

const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);

const ICONS: FolderView = FolderView {
    mode: ViewMode::Icons,
    icons: IconSize::Large,
};

struct TempHome(TestFolder);

impl TempHome {
    fn new(name: &str) -> Self {
        let path = TestFolder::new(&format!("folder-views-{name}"));
        Self(path)
    }

    fn core(&self) -> Core {
        let home = Home {
            folder: self.0.path().to_path_buf(),
            config: self.0.join("config"),
            state: self.0.join("state"),
            data: self.0.join("share"),
            drive_trashes: false,
        };
        Core(nova_files::spawn_with(IDLE_EXIT, Some(home), None, None).unwrap())
    }
}

impl TempHome {
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

fn next_views(events: &Receiver<Event>) -> FolderViews {
    let Event::Views(views) = events.recv_timeout(ANSWER_TIMEOUT).unwrap();
    views
}

#[test]
fn a_view_set_reaches_the_client_and_the_next_core() {
    let home = TempHome::new("set");
    let folder = Path::new("/home/me/Pictures").to_owned();
    {
        let core = home.core();
        let (sink, events) = mpsc::channel();
        let client = core.connector.connect(sink).unwrap();
        client.send(Command::Views).unwrap();
        assert!(next_views(&events).is_empty());
        client
            .send(Command::SetView {
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
    client.send(Command::Views).unwrap();
    assert_eq!(*next_views(&events), [(folder, ICONS)]);
}
