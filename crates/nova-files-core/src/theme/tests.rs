//! The store against real temp folders.

#![expect(
    clippy::unreadable_literal,
    reason = "colors are written as 0xRRGGBB, as in a theme file"
)]

use std::fs;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use nova_files_api::{Color, Palette, SettingChange, Settings};

use super::*;

/// How long a test waits for the theme before it counts as a failure.
const WAIT: Duration = Duration::from_secs(5);
/// A comment longer than a few lines, well under the cap.
const LONG_COMMENT: usize = 4096;

fn temp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("nova-files-theme-{name}-{}", std::process::id()));
    fs::remove_dir_all(&path).ok();
    fs::create_dir_all(config_watch::folder(&path)).unwrap();
    path
}

/// Hands each event to a channel.
struct Inbox(Mutex<Sender<Event>>);

impl EventSink for Inbox {
    fn deliver(&self, event: Event) -> Result<(), ClientGone> {
        lock(&self.0).send(event).map_err(|_| ClientGone)
    }
}

fn inbox() -> (Arc<dyn EventSink>, Receiver<Event>) {
    let (sender, events) = mpsc::channel();
    (Arc::new(Inbox(Mutex::new(sender))), events)
}

fn next(events: &Receiver<Event>) -> Arc<Theme> {
    match events.recv_timeout(WAIT).unwrap() {
        Event::Theme(theme) => theme,
        other => panic!("not the theme: {other:?}"),
    }
}

fn carbonfox(mode: Mode) -> Theme {
    Theme {
        mode,
        palette: defaults::carbonfox(mode),
    }
}

#[test]
fn without_a_file_the_theme_is_dark_carbonfox() {
    let config = temp("none");
    let store = Store::open(Some(&config));
    let (events, received) = inbox();
    store.listen(ClientId(0), &events);
    assert_eq!(*next(&received), carbonfox(Mode::Dark));
    drop(store);
    let store = Store::open(None);
    store.listen(ClientId(0), &events);
    assert_eq!(*next(&received), carbonfox(Mode::Dark));
    fs::remove_dir_all(config).ok();
}

#[test]
fn the_file_sets_its_colors_over_carbonfox() {
    let config = temp("file");
    let path = config_watch::folder(&config).join(FILE);
    fs::write(&path, "[dark]\nprimary = \"#123456\"\n").unwrap();
    let store = Store::open(Some(&config));
    let (events, received) = inbox();
    store.listen(ClientId(0), &events);
    let theme = next(&received);
    assert_eq!(theme.palette.primary, Color(0x123456));
    assert_eq!(theme.palette.bg, carbonfox(Mode::Dark).palette.bg);
    // An edit reaches the listening client.
    fs::write(&path, "[dark]\nprimary = \"#654321\"\n").unwrap();
    assert_eq!(next(&received).palette.primary, Color(0x654321));
    fs::remove_dir_all(config).ok();
}

#[test]
fn the_appearance_setting_picks_dark_or_light() {
    let config = temp("appearance");
    fs::write(
        config_watch::folder(&config).join(FILE),
        "[light]\nprimary = \"#ABCDEF\"\n",
    )
    .unwrap();
    let store = Store::open(Some(&config));
    let settings = SettingsStore::open(Some(&config));
    store.follow(&settings);
    let (events, received) = inbox();
    store.listen(ClientId(0), &events);
    assert_eq!(next(&received).mode, Mode::Dark);

    settings.change(SettingChange::Appearance(Appearance::Light));
    let light = next(&received);
    assert_eq!(light.mode, Mode::Light);
    let expected = Palette {
        primary: Color(0xABCDEF),
        ..defaults::carbonfox(Mode::Light)
    };
    assert_eq!(light.palette, expected);

    // Another setting changes nothing here.
    settings.change(SettingChange::Thumbnails(
        nova_files_api::Thumbnails::Hidden,
    ));
    // System is dark until the desktop's preference is known.
    settings.change(SettingChange::Appearance(Appearance::System));
    assert_eq!(next(&received).mode, Mode::Dark);
    assert!(received.recv_timeout(config_watch::QUIET * 3).is_err());
    drop(settings);
    fs::remove_dir_all(config).ok();
}

#[test]
fn a_client_that_left_hears_nothing_and_a_gone_store_stops_following() {
    let config = temp("left");
    let store = Store::open(Some(&config));
    let (events, received) = inbox();
    store.listen(ClientId(0), &events);
    next(&received);
    store.forget(ClientId(0));
    fs::write(
        config_watch::folder(&config).join(FILE),
        "[dark]\nbg = \"#000001\"\n",
    )
    .unwrap();
    assert!(received.recv_timeout(config_watch::QUIET * 5).is_err());

    let follower = Follower(Arc::downgrade(&store.shared));
    drop(store);
    let settings = Settings::default().with(SettingChange::Appearance(Appearance::Light));
    assert!(
        follower
            .deliver(Event::Settings(Arc::new(settings)))
            .is_err()
    );
    fs::remove_dir_all(config).ok();
}

#[test]
fn a_read_older_than_the_theme_is_done_again() {
    let config = temp("stale");
    let store = Store::open(Some(&config));
    let mut locked = lock(&store.shared);
    let version = locked.version;
    assert!(!locked.take(ThemeFile::default(), version));
    // A read from before that one comes in late: stale.
    assert!(locked.take(ThemeFile::default(), version));
    drop(locked);
    drop(store);
    fs::remove_dir_all(config).ok();
}

#[test]
fn an_unreadable_or_oversized_file_reads_as_carbonfox() {
    let config = temp("unreadable");
    let path = config_watch::folder(&config).join(FILE);
    // A long file a person wrote, well under the cap, is read.
    let comment = format!("# {}\n", "x".repeat(LONG_COMMENT));
    fs::write(&path, comment + "[dark]\nbg = \"#000001\"\n").unwrap();
    assert_eq!(read(&path).dark.colors.len(), 1);
    fs::write(&path, " ".repeat(MAX_BYTES + 1)).unwrap();
    assert_eq!(read(&path), ThemeFile::default());
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert_eq!(read(&path), ThemeFile::default());
    fs::remove_dir_all(config).ok();
}
