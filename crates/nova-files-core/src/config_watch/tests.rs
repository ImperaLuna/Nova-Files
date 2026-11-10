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
    let watch = ConfigWatch::start(folder.to_owned(), &["settings.toml"], move |changed| {
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

/// The first report other than `Changed::All`: a move and the save after
/// it may land in one burst or two.
fn first_files(reports: &Receiver<Changed>) -> Changed {
    loop {
        let changed = reports.recv_timeout(REPORT_TIMEOUT).unwrap();
        if changed != Changed::All {
            return changed;
        }
    }
}

#[test]
fn a_linked_file_saved_where_it_lives_is_reported_under_its_link() {
    let home = temp("linked");
    let config = home.join("nova-files");
    let dotfiles = home.join("dotfiles");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(&dotfiles).unwrap();
    fs::write(dotfiles.join("nova-settings.toml"), "").unwrap();
    std::os::unix::fs::symlink(
        dotfiles.join("nova-settings.toml"),
        config.join("settings.toml"),
    )
    .unwrap();
    let (_watch, reports) = watch(&config);
    // Another file next to it says nothing.
    fs::write(dotfiles.join("other.toml"), "").unwrap();
    assert!(reports.recv_timeout(QUIET * 3).is_err());
    fs::write(dotfiles.join("nova-settings.toml"), "appearance = \"dark\"").unwrap();
    assert_eq!(
        reports.recv_timeout(REPORT_TIMEOUT).unwrap(),
        files(&["settings.toml"])
    );
    fs::remove_dir_all(home).ok();
}

#[test]
fn a_link_pointed_elsewhere_is_followed_to_its_new_file() {
    let home = temp("relinked");
    let config = home.join("nova-files");
    let (old, new) = (home.join("old"), home.join("new"));
    for folder in [&config, &old, &new] {
        fs::create_dir_all(folder).unwrap();
        fs::write(folder.join("s.toml"), "").unwrap();
    }
    let link = config.join("settings.toml");
    std::os::unix::fs::symlink(old.join("s.toml"), &link).unwrap();
    let (_watch, reports) = watch(&config);
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(new.join("s.toml"), &link).unwrap();
    assert!(
        reports
            .recv_timeout(REPORT_TIMEOUT)
            .unwrap()
            .touches("settings.toml")
    );
    fs::write(old.join("s.toml"), "x").unwrap();
    assert!(reports.recv_timeout(QUIET * 3).is_err());
    fs::write(new.join("s.toml"), "x").unwrap();
    assert_eq!(
        reports.recv_timeout(REPORT_TIMEOUT).unwrap(),
        files(&["settings.toml"])
    );
    fs::remove_dir_all(home).ok();
}

#[test]
fn a_link_within_the_folder_leaves_the_folder_watched() {
    let config = temp("link-within");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("real.toml"), "").unwrap();
    std::os::unix::fs::symlink(config.join("real.toml"), config.join("settings.toml")).unwrap();
    let (_watch, reports) = watch(&config);
    fs::write(config.join("real.toml"), "x").unwrap();
    assert_eq!(
        reports.recv_timeout(REPORT_TIMEOUT).unwrap(),
        files(&["real.toml", "settings.toml"])
    );
    // Following the link again kept the folder's own watch.
    fs::write(config.join("theme.toml"), "").unwrap();
    assert_eq!(
        reports.recv_timeout(REPORT_TIMEOUT).unwrap(),
        files(&["theme.toml"])
    );
    fs::remove_dir_all(config).ok();
}

#[test]
fn a_link_to_the_folder_above_leaves_its_watch_for_a_swap() {
    let home = temp("link-above");
    let config = home.join("nova-files");
    let (old, new) = (home.join("old"), home.join("new"));
    fs::create_dir_all(&old).unwrap();
    fs::create_dir_all(&new).unwrap();
    fs::write(home.join("shared.toml"), "").unwrap();
    std::os::unix::fs::symlink(&old, &config).unwrap();
    std::os::unix::fs::symlink(home.join("shared.toml"), old.join("settings.toml")).unwrap();
    let (_watch, reports) = watch(&config);
    fs::write(home.join("shared.toml"), "x").unwrap();
    assert_eq!(
        reports.recv_timeout(REPORT_TIMEOUT).unwrap(),
        files(&["settings.toml"])
    );
    // Following the link again kept the watch for the folder's swap.
    fs::remove_file(&config).unwrap();
    std::os::unix::fs::symlink(&new, &config).unwrap();
    assert_eq!(reports.recv_timeout(REPORT_TIMEOUT).unwrap(), Changed::All);
    fs::remove_dir_all(home).ok();
}

#[test]
fn a_linked_folder_pointed_elsewhere_is_watched_there() {
    let home = temp("linked-folder");
    let config = home.join("nova-files");
    let (old, new) = (home.join("old"), home.join("new"));
    fs::create_dir_all(&old).unwrap();
    fs::create_dir_all(&new).unwrap();
    std::os::unix::fs::symlink(&old, &config).unwrap();
    let (_watch, reports) = watch(&config);
    fs::remove_file(&config).unwrap();
    std::os::unix::fs::symlink(&new, &config).unwrap();
    assert_eq!(reports.recv_timeout(REPORT_TIMEOUT).unwrap(), Changed::All);
    fs::write(new.join("settings.toml"), "").unwrap();
    assert_eq!(first_files(&reports), files(&["settings.toml"]));
    fs::remove_dir_all(home).ok();
}

/// Saves `file` every `gap` for `how_long`; says how many times the
/// machine held a save back past the quiet time, which may end a burst.
fn keep_saving(file: &Path, gap: Duration, how_long: Duration) -> u32 {
    // Never sent to: waiting on it is a pause.
    let (_never_sent, pause) = mpsc::channel::<()>();
    let start = Instant::now();
    let mut count = 0_u32;
    let mut late = 0_u32;
    let mut last = Instant::now();
    while start.elapsed() < how_long {
        count += 1;
        fs::write(file, count.to_string()).unwrap();
        if last.elapsed() >= QUIET {
            late += 1;
        }
        last = Instant::now();
        pause.recv_timeout(gap).ok();
    }
    late
}

#[test]
fn saves_closer_than_the_quiet_time_are_one_report() {
    let config = temp("busy");
    fs::create_dir_all(&config).unwrap();
    let (_watch, reports) = watch(&config);
    let late = keep_saving(&config.join("settings.toml"), QUIET / 4, LONGEST_WAIT / 3);
    let mut bursts = vec![reports.recv_timeout(REPORT_TIMEOUT).unwrap()];
    bursts.extend(std::iter::from_fn(|| reports.recv_timeout(QUIET * 3).ok()));
    assert!(
        bursts
            .iter()
            .all(|burst| *burst == files(&["settings.toml"]))
    );
    // One report, and one more only where a busy machine held a save
    // back past the quiet time.
    assert!(
        u32::try_from(bursts.len()).unwrap() <= 1 + late,
        "{} reports, {late} saves held back",
        bursts.len()
    );
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
fn lost_events_move_the_watch_to_where_the_folder_is_now() {
    let home = temp("overflow");
    let config = home.join("nova-files");
    let (old, new) = (home.join("old"), home.join("new"));
    fs::create_dir_all(&old).unwrap();
    fs::create_dir_all(&new).unwrap();
    std::os::unix::fs::symlink(&old, &config).unwrap();
    let mut watched = Watched {
        inotify: inotify::init(CreateFlags::CLOEXEC.union(CreateFlags::NONBLOCK)).unwrap(),
        target: config.clone(),
        files: Vec::new(),
        current: None,
        links: BTreeMap::new(),
    };
    watched.arm().unwrap();
    fs::remove_file(&config).unwrap();
    std::os::unix::fs::symlink(&new, &config).unwrap();
    // The swap's events were lost; only the overflow arrives.
    let lost = Event {
        wd: -1,
        mask: ReadFlags::QUEUE_OVERFLOW,
        name: None,
    };
    assert_eq!(watched.handle(&[lost]).unwrap(), Changed::All);
    let mut buffer = vec![MaybeUninit::uninit(); READ_BUFFER];
    read_events(&watched.inotify, &mut buffer).unwrap();
    fs::write(new.join("settings.toml"), "").unwrap();
    let events = read_events(&watched.inotify, &mut buffer).unwrap();
    assert_eq!(watched.handle(&events).unwrap(), files(&["settings.toml"]));
    fs::remove_dir_all(home).ok();
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
