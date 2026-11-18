//! The file's format, the list's order and cap, and who hears of changes.

use std::sync::mpsc;
use std::time::Duration;

use nova_files_api::ClientGone;

use super::*;

const WAIT: Duration = Duration::from_secs(10);

const ICONS: FolderView = FolderView {
    mode: ViewMode::Icons,
    icons: IconSize::Large,
};
const GALLERY: FolderView = FolderView {
    mode: ViewMode::Gallery,
    icons: IconSize::Smallest,
};

#[test]
fn the_file_reads_back_what_was_written() {
    let mut views = vec![
        (PathBuf::from("/home/me/Pictures"), ICONS),
        (
            PathBuf::from(OsString::from_vec(b"/home/me/\xff odd\nname".to_vec())),
            GALLERY,
        ),
    ];
    for (at, icons) in IconSize::ALL.into_iter().enumerate() {
        let view = FolderView {
            mode: ViewMode::Details,
            icons,
        };
        views.push((PathBuf::from(format!("/{at}")), view));
    }
    assert_eq!(parse(&to_bytes(&views)), views);
    assert_eq!(to_bytes(&[]), HEADER);
    assert_eq!(parse(HEADER), []);
}

#[test]
fn anything_else_reads_as_no_views() {
    let too_many = vec![(PathBuf::from("/a"), ICONS); MAX_FOLDER_VIEWS + 1];
    for bytes in [
        b"".to_vec(),
        b"i3/a\0".to_vec(),
        b"nova-files folder-views 2\ni3/a\0".to_vec(),
        b"nova-files folder-views 1\ni3/a".to_vec(),
        b"nova-files folder-views 1\n\0".to_vec(),
        b"nova-files folder-views 1\ni\0".to_vec(),
        b"nova-files folder-views 1\ni3\0".to_vec(),
        b"nova-files folder-views 1\ni3relative\0".to_vec(),
        b"nova-files folder-views 1\nx3/a\0".to_vec(),
        b"nova-files folder-views 1\ni0/a\0".to_vec(),
        b"nova-files folder-views 1\ni6/a\0".to_vec(),
        b"nova-files folder-views 1\ni3/a\0\0".to_vec(),
        to_bytes(&too_many),
    ] {
        assert_eq!(parse(&bytes), [], "{bytes:?}");
    }
    let full = vec![(PathBuf::from("/a"), ICONS); MAX_FOLDER_VIEWS];
    assert_eq!(parse(&to_bytes(&full)), full);
    let longest = PathBuf::from(format!("/{}", "a".repeat(MAX_PATH - 1)));
    assert!(fits(&longest));
    // The most the core writes is exactly the most it reads.
    let largest = to_bytes(&vec![(longest, ICONS); MAX_FOLDER_VIEWS]);
    assert_eq!(largest.len(), MAX_BYTES);
    assert!(!fits(&PathBuf::from(format!("/{}", "a".repeat(MAX_PATH)))));
}

fn folders(store: &Store) -> Vec<(String, FolderView)> {
    store
        .views
        .borrow()
        .iter()
        .map(|(folder, view)| (folder.display().to_string(), *view))
        .collect()
}

#[test]
fn a_view_set_goes_first_once_and_the_oldest_drop_off() {
    let store = Store::open(None);
    for (folder, view) in [
        ("/a", ICONS),
        ("/b", ICONS),
        ("/a", GALLERY),
        ("relative", ICONS),
    ] {
        store.set(PathBuf::from(folder), view);
    }
    let expected = vec![("/a".to_owned(), GALLERY), ("/b".to_owned(), ICONS)];
    assert_eq!(folders(&store), expected);

    for number in 0..=MAX_FOLDER_VIEWS {
        store.set(PathBuf::from(format!("/{number}")), ICONS);
    }
    let kept = folders(&store);
    assert_eq!(kept.len(), MAX_FOLDER_VIEWS);
    assert_eq!(kept[0].0, format!("/{MAX_FOLDER_VIEWS}"));
    assert_eq!(kept[MAX_FOLDER_VIEWS - 1].0, "/1");
}

/// A client that keeps the views it hears, while its receiver is alive.
struct Listener(mpsc::Sender<FolderViews>);

impl EventSink for Listener {
    fn deliver(&self, event: Event) -> Result<(), ClientGone> {
        match event {
            Event::FolderViews(views) => self.0.send(views).map_err(|_| ClientGone),
            _ => Ok(()),
        }
    }
}

fn listener() -> (Arc<dyn EventSink>, mpsc::Receiver<FolderViews>) {
    let (sender, heard) = mpsc::channel();
    (Arc::new(Listener(sender)), heard)
}

#[test]
fn listeners_hear_the_views_now_and_after_each_change() {
    let store = Store::open(None);
    store.set(PathBuf::from("/a"), ICONS);
    let (events, heard) = listener();
    let (gone, gone_heard) = listener();
    let (forgotten, forgotten_heard) = listener();

    store.listen(ClientId(1), &events);
    store.listen(ClientId(2), &gone);
    store.listen(ClientId(3), &forgotten);
    assert_eq!(heard.try_recv().unwrap().len(), 1);
    drop(gone_heard);
    forgotten_heard.try_recv().unwrap();
    store.forget(ClientId(3));

    // The same view again changes nothing.
    store.set(PathBuf::from("/a"), ICONS);
    assert!(heard.try_recv().is_err());

    store.set(PathBuf::from("/b"), GALLERY);
    let views = heard.try_recv().unwrap();
    assert_eq!(views[0], (PathBuf::from("/b"), GALLERY));
    assert!(forgotten_heard.try_recv().is_err());
    assert_eq!(
        store.listeners.borrow().keys().copied().collect::<Vec<_>>(),
        [ClientId(1)]
    );
}

#[test]
fn views_set_are_there_after_a_restart() {
    let state = std::env::temp_dir().join(format!("nova-files-views-{}", std::process::id()));
    std::fs::create_dir_all(state.join("nova-files")).unwrap();
    let store = Store::open(Some(&state));
    store.set(PathBuf::from("/a"), GALLERY);
    let wait = std::time::Instant::now();
    while folders(&Store::open(Some(&state))).is_empty() {
        assert!(wait.elapsed() < WAIT, "never saved");
        std::thread::yield_now();
    }

    let kept = folders(&Store::open(Some(&state)));
    std::fs::remove_dir_all(&state).ok();
    assert_eq!(kept, [("/a".to_owned(), GALLERY)]);
}
