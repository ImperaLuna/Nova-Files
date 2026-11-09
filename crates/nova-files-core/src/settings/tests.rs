//! The store's bookkeeping: versions, queued saves and reads put off
//! while saves are queued.

use std::sync::mpsc::Receiver;

use nova_files_api::{Appearance, Thumbnails};

use super::*;

fn temp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("nova-files-store-{name}-{}", std::process::id()));
    fs::remove_dir_all(&path).ok();
    fs::create_dir_all(&path).unwrap();
    path
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
    // Read before that change: stale.
    assert!(locked.take(Settings::default(), 0));
    assert_eq!(*locked.settings, dark());
    // Read after it: taken.
    assert!(!locked.take(Settings::default(), 1));
    assert_eq!(*locked.settings, Settings::default());
    drop(locked);
}

#[test]
fn a_read_that_changed_nothing_still_outdates_older_reads() {
    let shared = shared(None);
    let mut locked = lock(&shared);
    // The file was removed: defaults, as held.
    assert!(!locked.take(Settings::default(), 0));
    // A read from before the removal comes in late: stale.
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
    // A writer that stopped loses what it held: the file is the word.
    drop(queued);
    locked.read_when_saved = true;
    locked.queue(SettingChange::Thumbnails(Thumbnails::Hidden), &weak);
    assert_eq!(locked.unsaved, 0);
    assert!(!locked.read_when_saved);
    drop(locked);
}

#[test]
fn the_file_is_read_once_the_last_queued_save_is_done() {
    let folder = temp("saves");
    let path = folder.join(FILE);
    let shared = shared(Some(path.clone()));
    {
        let mut locked = lock(&shared);
        locked.unsaved = 2;
        // The watcher saw a change while the saves were queued.
        locked.read_when_saved = true;
    }
    let (queue, changes) = mpsc::channel();
    queue
        .send(SettingChange::Appearance(Appearance::Dark))
        .unwrap();
    drop(queue);
    save_each(&path, &changes, &Arc::downgrade(&shared));
    {
        // One of two saves is done: still waiting, nothing read.
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
    // Two saves, then the read that took the file's settings.
    assert_eq!(locked.version, 3);
    assert_eq!(
        *locked.settings,
        dark().with(SettingChange::Thumbnails(Thumbnails::Hidden))
    );
    drop(locked);
    fs::remove_dir_all(folder).ok();
}
