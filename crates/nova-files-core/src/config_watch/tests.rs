//! The watch against real temp folders.

use std::fs;
use std::sync::mpsc::{self, Receiver};

use super::*;

/// How long a test waits for a report before it counts as a failure.
const REPORT_TIMEOUT: Duration = Duration::from_secs(5);

fn temp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("nova-files-watch-{name}-{}", std::process::id()));
    fs::remove_dir_all(&path).ok();
    path
}

fn watch(folder: &Path) -> (ConfigWatch, Receiver<Changed>) {
    let (sender, reports) = mpsc::channel();
    let watch = ConfigWatch::start(folder.to_owned(), move |changed| {
        sender.send(changed.clone()).ok();
    })
    .unwrap();
    (watch, reports)
}

fn files(names: &[&str]) -> Changed {
    Changed::Files(names.iter().map(OsString::from).collect())
}

#[test]
fn a_save_by_rename_is_reported_once_with_its_file() {
    let config = temp("rename");
    fs::create_dir_all(&config).unwrap();
    let (_watch, reports) = watch(&config);
    fs::write(config.join("settings.toml.tmp"), "appearance = \"dark\"").unwrap();
    fs::rename(
        config.join("settings.toml.tmp"),
        config.join("settings.toml"),
    )
    .unwrap();
    let changed = reports.recv_timeout(REPORT_TIMEOUT).unwrap();
    assert_eq!(changed, files(&["settings.toml", "settings.toml.tmp"]));
    assert!(changed.touches("settings.toml"));
    assert!(!changed.touches("theme.toml"));
    // The burst came as one report.
    assert!(reports.recv_timeout(QUIET * 3).is_err());
    fs::remove_dir_all(config).ok();
}

#[test]
fn a_save_in_place_is_reported() {
    let config = temp("in-place");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("theme.toml"), "").unwrap();
    let (_watch, reports) = watch(&config);
    fs::write(config.join("theme.toml"), "name = \"x\"").unwrap();
    assert_eq!(
        reports.recv_timeout(REPORT_TIMEOUT).unwrap(),
        files(&["theme.toml"])
    );
    fs::remove_dir_all(config).ok();
}

#[test]
fn a_folder_made_later_is_watched_once_it_appears() {
    let home = temp("later");
    let config = home.join("config/nova-files");
    fs::create_dir_all(&home).unwrap();
    let (_watch, reports) = watch(&config);
    // Two levels appear: the watch follows them down.
    fs::create_dir_all(&config).unwrap();
    assert_eq!(reports.recv_timeout(REPORT_TIMEOUT).unwrap(), Changed::All);
    fs::write(config.join("settings.toml"), "").unwrap();
    let mut changed = reports.recv_timeout(REPORT_TIMEOUT).unwrap();
    // The folder's arrival and the file may land in one burst or two.
    while changed == Changed::All {
        changed = reports.recv_timeout(REPORT_TIMEOUT).unwrap();
    }
    assert_eq!(changed, files(&["settings.toml"]));
    fs::remove_dir_all(home).ok();
}

#[test]
fn above_the_folder_only_the_way_down_matters() {
    let home = temp("above");
    let config = home.join("nova-files");
    fs::create_dir_all(&home).unwrap();
    let (_watch, reports) = watch(&config);
    // Another app's config next to ours says nothing.
    fs::create_dir_all(home.join("other-app")).unwrap();
    fs::write(home.join("other-app.conf"), "").unwrap();
    assert!(reports.recv_timeout(QUIET * 3).is_err());
    fs::create_dir_all(&config).unwrap();
    assert_eq!(reports.recv_timeout(REPORT_TIMEOUT).unwrap(), Changed::All);
    fs::remove_dir_all(home).ok();
}

#[test]
fn a_folder_that_goes_away_reports_everything_and_is_found_again() {
    let home = temp("gone");
    let config = home.join("nova-files");
    fs::create_dir_all(&config).unwrap();
    let (_watch, reports) = watch(&config);
    fs::remove_dir_all(&config).unwrap();
    assert_eq!(reports.recv_timeout(REPORT_TIMEOUT).unwrap(), Changed::All);
    fs::create_dir_all(&config).unwrap();
    assert_eq!(reports.recv_timeout(REPORT_TIMEOUT).unwrap(), Changed::All);
    fs::write(config.join("settings.toml"), "").unwrap();
    assert_eq!(
        reports.recv_timeout(REPORT_TIMEOUT).unwrap(),
        files(&["settings.toml"])
    );
    fs::remove_dir_all(home).ok();
}

/// Saves `file` every `gap` for `how_long`.
fn keep_saving(file: &Path, gap: Duration, how_long: Duration) {
    // Never sent to: waiting on it is a pause.
    let (_never_sent, pause) = mpsc::channel::<()>();
    let start = Instant::now();
    let mut count = 0_u32;
    while start.elapsed() < how_long {
        count += 1;
        fs::write(file, count.to_string()).unwrap();
        pause.recv_timeout(gap).ok();
    }
}

#[test]
fn saves_closer_than_the_quiet_time_are_one_report() {
    let config = temp("busy");
    fs::create_dir_all(&config).unwrap();
    let (_watch, reports) = watch(&config);
    keep_saving(&config.join("settings.toml"), QUIET / 4, LONGEST_WAIT / 3);
    assert_eq!(
        reports.recv_timeout(REPORT_TIMEOUT).unwrap(),
        files(&["settings.toml"])
    );
    assert!(reports.recv_timeout(QUIET * 3).is_err());
    fs::remove_dir_all(config).ok();
}

#[test]
fn a_folder_that_never_goes_quiet_is_still_reported() {
    let config = temp("never-quiet");
    fs::create_dir_all(&config).unwrap();
    let (_watch, reports) = watch(&config);
    let (done, finished) = mpsc::channel();
    let file = config.join("settings.toml");
    let saver = thread::spawn(move || {
        keep_saving(&file, QUIET / 4, LONGEST_WAIT * 2);
        done.send(()).ok();
    });
    // Reported while the saves go on.
    assert_eq!(
        reports.recv_timeout(REPORT_TIMEOUT).unwrap(),
        files(&["settings.toml"])
    );
    assert!(
        finished.try_recv().is_err(),
        "reported only after the saves"
    );
    saver.join().unwrap();
    fs::remove_dir_all(config).ok();
}

#[test]
fn a_single_save_is_reported_without_the_longest_wait() {
    let config = temp("prompt");
    fs::create_dir_all(&config).unwrap();
    let (_watch, reports) = watch(&config);
    fs::write(config.join("settings.toml"), "").unwrap();
    assert!(reports.recv_timeout(LONGEST_WAIT * 3 / 4).is_ok());
    fs::remove_dir_all(config).ok();
}

#[test]
fn nothing_is_reported_once_the_watch_is_dropped() {
    let config = temp("dropped");
    fs::create_dir_all(&config).unwrap();
    let (watch, reports) = watch(&config);
    drop(watch);
    fs::write(config.join("settings.toml"), "").unwrap();
    assert!(reports.recv_timeout(QUIET * 3).is_err());
    fs::remove_dir_all(config).ok();
}

#[test]
fn bursts_add_up_and_everything_wins() {
    let mut changed = files(&["a"]);
    changed.add(files(&["b"]));
    assert_eq!(changed, files(&["a", "b"]));
    changed.add(Changed::All);
    assert_eq!(changed, Changed::All);
    changed.add(files(&["c"]));
    assert_eq!(changed, Changed::All);
    assert!(files(&[]).is_empty());
    assert!(!Changed::All.is_empty());
}
