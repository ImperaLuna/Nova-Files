use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use crate::common::Core;
use nova_files::{Home, IDLE_EXIT};
use nova_files_api::{Color, Connection, DesktopLook, Mode, Theme};
use nova_files_look_api as look_api;
use nova_files_settings_api::{
    self as settings_api, Appearance, SettingChange, Settings, Thumbnails,
};
use nova_files_test_folder::TestFolder;

const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);
const NOTHING_MORE: Duration = Duration::from_millis(600);
const SHARED_MODE: u32 = 0o644;
const PERMISSION_BITS: u32 = 0o7777;
const LONG_COMMENT: usize = 4096;

struct TempHome(TestFolder);

impl TempHome {
    fn new(name: &str) -> Self {
        let path = TestFolder::new(&format!("settings-{name}"));
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

fn listen(core: &Core) -> (Connection, Receiver<settings_api::Event>, Settings) {
    let (sink, events) = mpsc::channel();
    let connection = core.connector.connect(sink).unwrap();
    connection.send(settings_api::Command::Watch).unwrap();
    let first = next(&events);
    (connection, events, first)
}

fn next(events: &Receiver<settings_api::Event>) -> Settings {
    match events.recv_timeout(ANSWER_TIMEOUT) {
        Ok(settings_api::Event::Settings(settings)) => Arc::unwrap_or_clone(settings),
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
    let (sink, quiet) = mpsc::channel::<settings_api::Event>();
    let _quiet = core.connector.connect(sink).unwrap();

    first
        .send(settings_api::Command::Change(SettingChange::Thumbnails(
            Thumbnails::Hidden,
        )))
        .unwrap();

    assert_eq!(next(&first_events).thumbnails, Thumbnails::Hidden);
    assert_eq!(next(&second_events).thumbnails, Thumbnails::Hidden);
    wait_for_file(
        &home.file(),
        "# mine\nappearance = \"dark\"\nthumbnails = false\n",
    );
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
        .send(settings_api::Command::Change(SettingChange::Thumbnails(
            Thumbnails::Hidden,
        )))
        .unwrap();
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
        .send(settings_api::Command::Change(SettingChange::Appearance(
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
        .send(settings_api::Command::Change(SettingChange::Appearance(
            Appearance::Dark,
        )))
        .unwrap();
    assert_eq!(next(&events).appearance, Appearance::Dark);
    wait_for_file(&real, "appearance = \"dark\"\n");
    assert!(fs::symlink_metadata(home.file()).unwrap().is_symlink());
    let mode = fs::metadata(&real).unwrap().permissions().mode() & PERMISSION_BITS;
    assert_eq!(mode, SHARED_MODE);
}

fn next_theme(events: &Receiver<look_api::Event>) -> Arc<Theme> {
    match events.recv_timeout(ANSWER_TIMEOUT) {
        Ok(look_api::Event::Theme(theme)) => theme,
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
    theme_client.send(look_api::Command::Watch).unwrap();
    assert_eq!(next_theme(&theme_events).mode, Mode::Dark);

    let (settings_client, _settings_events, _) = listen(&core);
    settings_client
        .send(settings_api::Command::Change(SettingChange::Appearance(
            Appearance::Light,
        )))
        .unwrap();
    let light = next_theme(&theme_events);
    assert_eq!(light.mode, Mode::Light);
    assert_eq!(light.palette.primary, Color(0x00C0_FFEE));
}

#[test]
fn the_desktops_look_reaches_the_theme() {
    let home = TempHome::new("desktop-look");
    let core = home.core();
    let (themes, theme_events) = mpsc::channel();
    let theme_client = core.connector.connect(themes).unwrap();
    theme_client.send(look_api::Command::Watch).unwrap();
    assert_eq!(next_theme(&theme_events).mode, Mode::Dark);

    theme_client
        .send(look_api::Command::DesktopLook(DesktopLook {
            mode: Some(Mode::Light),
            accent: None,
        }))
        .unwrap();
    assert_eq!(next_theme(&theme_events).mode, Mode::Light);
}
