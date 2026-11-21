use std::fs;
use std::sync::mpsc::Receiver;

use nova_files_settings_api::{Appearance, Thumbnails};
use nova_files_test_folder::TestFolder;

use super::*;

fn temp(name: &str) -> (TestFolder, PathBuf) {
    let folder = TestFolder::new(&format!("store-{name}"));
    let path = folder.path().to_path_buf();
    (folder, path)
}

fn shared(path: Option<PathBuf>) -> Arc<Mutex<Shared>> {
    Arc::new(Mutex::new(Shared {
        settings: Arc::default(),
        listeners: BTreeMap::new(),
        path,
        writer: None,
        unsaved: 0,
        read_when_saved: false,
        version: 0,
    }))
}

fn dark() -> Settings {
    Settings::default().with(SettingChange::Appearance(Appearance::Dark))
}

#[test]
fn every_new_value_is_a_new_version() {
    let shared = shared(None);
    let mut locked = lock(&shared);
    locked.set(dark());
    assert_eq!(locked.version, 1);
    locked.set(Settings::default());
    assert_eq!(locked.version, 2);
    drop(locked);
}

#[test]
fn a_read_while_saves_are_queued_waits_for_them() {
    let shared = shared(None);
    let mut locked = lock(&shared);
    locked.unsaved = 1;
    assert!(!locked.take(dark(), 0));
    assert!(locked.read_when_saved);
    assert_eq!(*locked.settings, Settings::default());
    drop(locked);
}

#[test]
fn a_read_older_than_the_settings_is_done_again() {
    let shared = shared(None);
    let mut locked = lock(&shared);
    locked.set(dark());
    assert!(locked.take(Settings::default(), 0));
    assert_eq!(*locked.settings, dark());
    assert!(!locked.take(Settings::default(), 1));
    assert_eq!(*locked.settings, Settings::default());
    drop(locked);
}

#[test]
fn a_read_that_changed_nothing_still_outdates_older_reads() {
    let shared = shared(None);
    let mut locked = lock(&shared);
    assert!(!locked.take(Settings::default(), 0));
    assert!(locked.take(dark(), 0));
    assert_eq!(*locked.settings, Settings::default());
    drop(locked);
}

#[test]
fn a_queued_change_counts_as_unsaved() {
    let shared = shared(Some(PathBuf::from("settings.toml")));
    let (queue, queued): (Sender<SettingChange>, Receiver<SettingChange>) = mpsc::channel();
    let mut locked = lock(&shared);
    locked.writer = Some(queue);
    let weak = Arc::downgrade(&shared);
    locked.queue(SettingChange::Thumbnails(Thumbnails::Hidden), &weak);
    locked.queue(SettingChange::Thumbnails(Thumbnails::Shown), &weak);
    assert_eq!(locked.unsaved, 2);
    assert_eq!(queued.try_iter().count(), 2);
    drop(queued);
    locked.read_when_saved = true;
    locked.queue(SettingChange::Thumbnails(Thumbnails::Hidden), &weak);
    assert_eq!(locked.unsaved, 0);
    assert!(!locked.read_when_saved);
    drop(locked);
}

#[test]
fn the_file_is_read_once_the_last_queued_save_is_done() {
    let (_held, folder) = temp("saves");
    let path = folder.join(FILE);
    let shared = shared(Some(path.clone()));
    {
        let mut locked = lock(&shared);
        locked.unsaved = 2;
        locked.read_when_saved = true;
    }
    let (queue, changes) = mpsc::channel();
    queue
        .send(SettingChange::Appearance(Appearance::Dark))
        .unwrap();
    drop(queue);
    save_each(&path, &changes, &Arc::downgrade(&shared));
    {
        let locked = lock(&shared);
        assert_eq!(locked.unsaved, 1);
        assert!(locked.read_when_saved);
        assert_eq!(*locked.settings, Settings::default());
    }
    let (queue, changes) = mpsc::channel();
    queue
        .send(SettingChange::Thumbnails(Thumbnails::Hidden))
        .unwrap();
    drop(queue);
    save_each(&path, &changes, &Arc::downgrade(&shared));
    let locked = lock(&shared);
    assert_eq!(locked.unsaved, 0);
    assert!(!locked.read_when_saved);
    assert_eq!(locked.version, 3);
    assert_eq!(
        *locked.settings,
        dark().with(SettingChange::Thumbnails(Thumbnails::Hidden))
    );
    drop(locked);
}

#[test]
fn the_store_holds_what_the_file_says() {
    let (_config_folder, config) = temp("current");
    let folder = config_watch::folder(&config);
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join(FILE), "thumbnails = false\n").unwrap();
    let store = Store::open(Some(&config));
    assert_eq!(store.current().thumbnails, Thumbnails::Hidden);
    drop(store);
}
