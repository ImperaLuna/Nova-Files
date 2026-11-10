//! Settings and the theme through the core: read from their files, changed
//! by a client or on disk, and sent to every client that asked, in a temp
//! home.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use nova_files_api::{
    Appearance, Color, Command, Connection, Event, Mode, SettingChange, Settings, Theme, Thumbnails,
};
use nova_files_core::{Home, IDLE_EXIT, RunningCore};

/// How long a test waits for the core before it counts as a failure.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);
/// Long enough for the watcher's quiet wait and a read: no event in this
/// time means none is coming.
const NOTHING_MORE: Duration = Duration::from_millis(600);
/// A mode the user gave their own file, readable by others.
const SHARED_MODE: u32 = 0o644;
const PERMISSION_BITS: u32 = 0o7777;
/// A comment longer than a few lines, well under the cap.
const LONG_COMMENT: usize = 4096;

struct TempHome(PathBuf);

impl TempHome {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("nova-files-settings-{name}-{}", std::process::id()));
        fs::remove_dir_all(&path).ok();
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn file(&self) -> PathBuf {
        self.0.join("config/nova-files/settings.toml")
    }

    fn write(&self, text: &str) {
        let file = self.file();
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let temporary = file.with_extension("tmp");
        fs::write(&temporary, text).unwrap();
        fs::rename(temporary, file).unwrap();
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

impl Drop for TempHome {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// A connected client that asked for the settings, with the first answer.
fn listen(core: &RunningCore) -> (Connection, Receiver<Event>, Settings) {
    let (sink, events) = mpsc::channel();
    let connection = core.connector.connect(sink).unwrap();
    connection.send(Command::Settings).unwrap();
    let first = next(&events);
    (connection, events, first)
}

fn next(events: &Receiver<Event>) -> Settings {
    match events.recv_timeout(ANSWER_TIMEOUT) {
        Ok(Event::Settings(settings)) => Arc::unwrap_or_clone(settings),
        other => panic!("expected settings, got {other:?}"),
    }
}

fn wait_for_file(path: &Path, wanted: &str) {
    let start = std::time::Instant::now();
    while fs::read_to_string(path).ok().as_deref() != Some(wanted) {
        assert!(start.elapsed() < ANSWER_TIMEOUT, "never saved: {wanted}");
        std::thread::yield_now();
    }
}

#[test]
fn a_new_user_gets_the_defaults_and_no_file() {
    let home = TempHome::new("new");
    let core = home.core();
    let (_connection, _events, settings) = listen(&core);
    assert_eq!(settings, Settings::default());
    assert!(!home.file().exists());
}

#[test]
fn the_file_is_read_and_a_bad_key_falls_back_alone() {
    let home = TempHome::new("read");
    home.write("appearance = \"purple\"\nthumbnails = false\n");
    let core = home.core();
    let (_connection, _events, settings) = listen(&core);
    assert_eq!(settings.appearance, Appearance::System);
    assert_eq!(settings.thumbnails, Thumbnails::Hidden);
}

#[test]
fn a_change_reaches_every_listening_client_and_is_saved_once() {
    let home = TempHome::new("change");
    home.write("# mine\nappearance = \"dark\"\n");
    let core = home.core();
    let (first, first_events, _) = listen(&core);
    let (_second, second_events, _) = listen(&core);
    // A client that never asked hears nothing.
    let (sink, quiet) = mpsc::channel();
    let _quiet = core.connector.connect(sink).unwrap();

    first
        .send(Command::ChangeSetting(SettingChange::Thumbnails(
            Thumbnails::Hidden,
        )))
        .unwrap();

    assert_eq!(next(&first_events).thumbnails, Thumbnails::Hidden);
    assert_eq!(next(&second_events).thumbnails, Thumbnails::Hidden);
    wait_for_file(
        &home.file(),
        "# mine\nappearance = \"dark\"\nthumbnails = false\n",
    );
    // The save coming back through the watcher sends nothing more.
    assert!(first_events.recv_timeout(NOTHING_MORE).is_err());
    assert!(quiet.recv_timeout(NOTHING_MORE).is_err());
}

#[test]
fn a_client_that_left_hears_nothing_more() {
    let home = TempHome::new("left");
    let core = home.core();
    let (staying, _staying_events, _) = listen(&core);
    let (leaving, left_events, _) = listen(&core);
    drop(leaving);
    staying
        .send(Command::ChangeSetting(SettingChange::Thumbnails(
            Thumbnails::Hidden,
        )))
        .unwrap();
    // Its events still have somewhere to go, but none come.
    assert!(left_events.recv_timeout(NOTHING_MORE).is_err());
}

#[test]
fn a_long_file_is_read_up_to_its_cap() {
    let home = TempHome::new("long");
    let comment = format!("# {}\n", "x".repeat(LONG_COMMENT));
    home.write(&format!("{comment}appearance = \"dark\"\n"));
    let core = home.core();
    let (_connection, _events, settings) = listen(&core);
    assert_eq!(settings.appearance, Appearance::Dark);
    // Past the cap, 64 KiB, it isn't read at all.
    let huge = format!("# {}\n", "x".repeat(64 * 1024));
    home.write(&format!("{huge}appearance = \"dark\"\n"));
    let core = home.core();
    let (_connection, _events, settings) = listen(&core);
    assert_eq!(settings, Settings::default());
}

#[test]
fn setting_a_value_already_set_sends_nothing() {
    let home = TempHome::new("same");
    let core = home.core();
    let (connection, events, _) = listen(&core);
    connection
        .send(Command::ChangeSetting(SettingChange::Appearance(
            Appearance::System,
        )))
        .unwrap();
    assert!(events.recv_timeout(NOTHING_MORE).is_err());
    assert!(!home.file().exists());
}

#[test]
fn an_edit_on_disk_reaches_the_clients() {
    let home = TempHome::new("disk");
    let core = home.core();
    let (_connection, events, _) = listen(&core);
    // The folder doesn't exist yet: the watch follows it down.
    home.write("appearance = \"light\"\n");
    assert_eq!(next(&events).appearance, Appearance::Light);
    home.write("appearance = \"light\"\n[keys]\n\"go.back\" = [\"alt-left\"]\n");
    assert_eq!(next(&events).keys["go.back"], ["alt-left"]);
    fs::remove_file(home.file()).unwrap();
    assert_eq!(next(&events), Settings::default());
}

#[test]
fn a_linked_file_is_saved_where_it_points() {
    let home = TempHome::new("link");
    let real = home.0.join("dotfiles/settings.toml");
    fs::create_dir_all(real.parent().unwrap()).unwrap();
    fs::write(&real, "").unwrap();
    fs::set_permissions(&real, fs::Permissions::from_mode(SHARED_MODE)).unwrap();
    fs::create_dir_all(home.file().parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&real, home.file()).unwrap();
    let core = home.core();
    let (connection, events, _) = listen(&core);
    connection
        .send(Command::ChangeSetting(SettingChange::Appearance(
            Appearance::Dark,
        )))
        .unwrap();
    assert_eq!(next(&events).appearance, Appearance::Dark);
    wait_for_file(&real, "appearance = \"dark\"\n");
    assert!(fs::symlink_metadata(home.file()).unwrap().is_symlink());
    // The user's file keeps its mode.
    let mode = fs::metadata(&real).unwrap().permissions().mode() & PERMISSION_BITS;
    assert_eq!(mode, SHARED_MODE);
}

fn next_theme(events: &Receiver<Event>) -> Arc<Theme> {
    match events.recv_timeout(ANSWER_TIMEOUT) {
        Ok(Event::Theme(theme)) => theme,
        other => panic!("expected the theme, got {other:?}"),
    }
}

#[test]
fn the_theme_follows_its_file_and_the_appearance_setting() {
    let home = TempHome::new("theme");
    let theme_file = home.file().with_file_name("theme.toml");
    fs::create_dir_all(theme_file.parent().unwrap()).unwrap();
    fs::write(&theme_file, "[light]\nprimary = \"#C0FFEE\"\n").unwrap();
    let core = home.core();
    let (themes, theme_events) = mpsc::channel();
    let theme_client = core.connector.connect(themes).unwrap();
    theme_client.send(Command::Theme).unwrap();
    assert_eq!(next_theme(&theme_events).mode, Mode::Dark);

    let (settings_client, settings_events, _) = listen(&core);
    settings_client
        .send(Command::ChangeSetting(SettingChange::Appearance(
            Appearance::Light,
        )))
        .unwrap();
    let light = next_theme(&theme_events);
    assert_eq!(light.mode, Mode::Light);
    assert_eq!(light.palette.primary, Color(0x00C0_FFEE));
    // Only clients that asked hear about the theme.
    assert!(
        !settings_events
            .try_iter()
            .any(|event| matches!(event, Event::Theme(_)))
    );
}
