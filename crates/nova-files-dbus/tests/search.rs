//! The search interface against a private D-Bus daemon and a real core
//! whose home and index are temp folders: stages arrive as signals for the
//! caller only, `Open`
//! hands folders to a window and files it finds no app for to a stand-in
//! portal, and
//! `ShowInFolder` opens the holding folder.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test setup should fail loudly"
)]

use std::collections::HashMap;
use std::fs::{self, File};
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use nova_files_core::{Home, IDLE_EXIT, IndexPlace, RunningCore};
use nova_files_dbus::{
    ACTIVATION_TOKEN, APP_ID, PORTAL_NAME, PORTAL_PATH, SEARCH_INTERFACE, Started,
};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

mod support;

use support::{PrivateBus, WAIT};

/// How long a test waits for something that should not happen.
const QUIET: Duration = Duration::from_millis(300);
const LIMIT: u32 = 50;
const TOKEN: &str = "launcher-token";

/// One `Found` signal: id, hits, done.
type Found = (u64, Vec<(String, String)>, bool);

/// A home with a few files and a cache folder, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "nova-files-dbus-search-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let scratch = Self(std::env::temp_dir().join(name));
        fs::create_dir_all(scratch.home().join("Documents")).unwrap();
        fs::write(scratch.file(), b"report").unwrap();
        fs::write(scratch.untyped(), b"notes").unwrap();
        scratch
    }

    fn home(&self) -> PathBuf {
        self.0.join("home")
    }

    fn file(&self) -> PathBuf {
        self.home().join("Documents/report.pdf")
    }

    /// A file whose name says no type, so the core finds no app for it and
    /// leaves it to the portal, whatever apps the machine running the test
    /// has.
    fn untyped(&self) -> PathBuf {
        self.home().join("Documents/notes")
    }

    fn core(&self, indexed: bool, idle_exit: Duration) -> RunningCore {
        let home = Home {
            folder: self.home(),
            config: self.0.join("config"),
        };
        let place = indexed.then(|| IndexPlace {
            folder: self.0.join("cache"),
        });
        nova_files_core::spawn_with(idle_exit, Some(home), None, place).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// The service on a private bus, with each window it is asked for on
/// `windows`.
struct Service {
    bus: PrivateBus,
    _started: Started,
    windows: Receiver<Option<PathBuf>>,
    /// Gets a message once the core has stopped.
    core_ended: Receiver<()>,
}

fn service(scratch: &Scratch, indexed: bool) -> Service {
    service_with(scratch, indexed, IDLE_EXIT)
}

fn service_with(scratch: &Scratch, indexed: bool, idle_exit: Duration) -> Service {
    let bus = PrivateBus::start();
    let RunningCore { connector, thread } = scratch.core(indexed, idle_exit);
    let (sender, windows) = mpsc::channel();
    let started = nova_files_dbus::start(bus.bus(), connector, move |folder| {
        sender.send(folder).ok();
    })
    .unwrap();
    let (ended, core_ended) = mpsc::channel();
    std::thread::spawn(move || {
        thread.join().ok();
        ended.send(()).ok();
    });
    Service {
        bus,
        _started: started,
        windows,
        core_ended,
    }
}

/// A short idle time, for tests of what keeps the core up.
const SHORT_IDLE: Duration = Duration::from_secs(1);

/// A launcher: a bus connection, the search interface, and the signals it
/// receives.
struct Launcher {
    search: Proxy<'static>,
    found: Receiver<Found>,
    failed: Receiver<(u64, String)>,
}

impl Launcher {
    fn new(bus: &PrivateBus) -> Self {
        let connection = connect(bus);
        let search = Proxy::new(
            &connection,
            APP_ID,
            nova_files_dbus::object_path().unwrap(),
            SEARCH_INTERFACE,
        )
        .unwrap();
        let found = forward(&search, "Found", |body| body.deserialize().unwrap());
        let failed = forward(&search, "Failed", |body| body.deserialize().unwrap());
        Self {
            search,
            found,
            failed,
        }
    }

    fn search(&self, id: u64, query: &str) {
        self.search
            .call::<_, _, ()>("Search", &(id, query, LIMIT))
            .unwrap();
    }

    /// Every stage of search `id`, up to the one marked done.
    fn stages(&self, id: u64) -> Vec<Vec<(String, String)>> {
        let mut stages = Vec::new();
        loop {
            let (found, hits, done) = self.found.recv_timeout(WAIT).unwrap();
            assert_eq!(found, id);
            stages.push(hits);
            if done {
                return stages;
            }
        }
    }

    fn call(&self, method: &str, path: &Path, token: Option<&str>) -> zbus::Result<()> {
        let mut platform_data: HashMap<&str, Value<'_>> = HashMap::new();
        if let Some(token) = token {
            platform_data.insert(ACTIVATION_TOKEN, Value::from(token));
        }
        let path = path.to_str().unwrap();
        self.search.call::<_, _, ()>(method, &(path, platform_data))
    }
}

fn connect(bus: &PrivateBus) -> Connection {
    zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .method_timeout(WAIT)
        .build()
        .unwrap()
}

/// The bodies of `signal` from `proxy`, on a channel, turned into `T` by
/// `read`. Subscribed before it returns.
fn forward<T: Send + 'static>(
    proxy: &Proxy<'static>,
    signal: &'static str,
    read: fn(&zbus::message::Body) -> T,
) -> Receiver<T> {
    let signals = proxy.receive_signal(signal).unwrap();
    let (sender, received) = mpsc::channel();
    std::thread::spawn(move || {
        for message in signals {
            if sender.send(read(&message.body())).is_err() {
                return;
            }
        }
    });
    received
}

fn hit(path: &str, matched: &str) -> (String, String) {
    (path.to_owned(), matched.to_owned())
}

#[test]
fn stages_come_as_signals_to_the_caller_only() {
    let scratch = Scratch::new();
    let service = service(&scratch, true);
    let launcher = Launcher::new(&service.bus);
    let bystander = Launcher::new(&service.bus);
    launcher.search(1, "report");
    let path = scratch.file().to_str().unwrap().to_owned();
    assert_eq!(launcher.stages(1), [vec![hit(&path, "exact")], vec![]]);
    launcher.search(2, "rprt");
    assert_eq!(launcher.stages(2), [vec![], vec![hit(&path, "in-order")]]);
    assert!(bystander.found.recv_timeout(QUIET).is_err());
}

#[test]
fn every_match_kind_has_a_name() {
    let scratch = Scratch::new();
    for name in ["report-2024.txt", "myreports.txt", "rpeort.txt"] {
        fs::write(scratch.home().join(name), b"").unwrap();
    }
    let service = service(&scratch, true);
    let launcher = Launcher::new(&service.bus);
    launcher.search(1, "report");
    let kinds: Vec<String> = launcher
        .stages(1)
        .concat()
        .into_iter()
        .map(|(_, kind)| kind)
        .collect();
    assert_eq!(kinds, ["exact", "word-start", "contains", "one-typo"]);
}

#[test]
fn a_search_that_cant_run_says_why() {
    let scratch = Scratch::new();
    let service = service(&scratch, false);
    let launcher = Launcher::new(&service.bus);
    launcher.search(1, "report");
    assert_eq!(
        launcher.failed.recv_timeout(WAIT),
        Ok((1, "no-index".to_owned()))
    );
    assert!(launcher.found.recv_timeout(QUIET).is_err());
}

/// Puts a FIFO where the saved index goes: the core's first search then
/// waits, opening it, until [`release`] opens the other end.
fn block_the_index(scratch: &Scratch) -> PathBuf {
    let cache = scratch.0.join("cache");
    fs::create_dir_all(&cache).unwrap();
    let fifo = IndexPlace { folder: cache }.index();
    fifo_at(&fifo);
    fifo
}

fn fifo_at(path: &Path) {
    rustix::fs::mknodat(
        rustix::fs::CWD,
        path,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        0,
    )
    .unwrap();
}

/// Lets the core read the blocked index once it is waiting on it: it finds
/// it empty and crawls.
fn release(fifo: &Path) {
    use rustix::fs::{Mode, OFlags};
    let started = Instant::now();
    // Without a reader yet, a writer that doesn't wait is refused.
    let flags = OFlags::WRONLY.union(OFlags::NONBLOCK);
    while matches!(
        rustix::fs::open(fifo, flags, Mode::empty()),
        Err(rustix::io::Errno::NXIO)
    ) {
        assert!(started.elapsed() < WAIT, "the core never read the index");
    }
}

#[test]
fn a_cancelled_search_sends_nothing() {
    let scratch = Scratch::new();
    let fifo = block_the_index(&scratch);
    let service = service(&scratch, true);
    let launcher = Launcher::new(&service.bus);
    launcher.search(1, "report");
    launcher
        .search
        .call::<_, _, ()>("Cancel", &(1_u64,))
        .unwrap();
    release(&fifo);
    assert!(launcher.found.recv_timeout(QUIET).is_err());
    launcher.search(2, "report");
    assert_eq!(launcher.stages(2).len(), 2);
}

#[test]
fn a_search_ends_when_its_caller_leaves() {
    let scratch = Scratch::new();
    let _fifo = block_the_index(&scratch);
    let service = service_with(&scratch, true, SHORT_IDLE);
    // No signal streams, so dropping it leaves the bus.
    let caller = connect(&service.bus);
    let search = Proxy::new(
        &caller,
        APP_ID,
        nova_files_dbus::object_path().unwrap(),
        SEARCH_INTERFACE,
    )
    .unwrap();
    // Waits on the index for good: only the caller leaving ends it.
    search
        .call::<_, _, ()>("Search", &(1_u64, "report", LIMIT))
        .unwrap();
    drop((search, caller));
    service.core_ended.recv_timeout(WAIT).unwrap();
}

#[test]
fn a_newer_search_replaces_the_older_one() {
    let scratch = Scratch::new();
    let fifo = block_the_index(&scratch);
    let service = service(&scratch, true);
    let launcher = Launcher::new(&service.bus);
    launcher.search(1, "rep");
    launcher.search(2, "report");
    release(&fifo);
    assert_eq!(launcher.stages(2).len(), 2);
    assert!(launcher.found.recv_timeout(QUIET).is_err());
}

#[test]
fn cancelling_a_finished_search_is_harmless() {
    let scratch = Scratch::new();
    let service = service(&scratch, true);
    let launcher = Launcher::new(&service.bus);
    launcher.search(1, "report");
    assert_eq!(launcher.stages(1).len(), 2);
    launcher
        .search
        .call::<_, _, ()>("Cancel", &(1_u64,))
        .unwrap();
    launcher.search(2, "report");
    assert_eq!(launcher.stages(2).len(), 2);
    assert!(launcher.found.recv_timeout(QUIET).is_err());
}

#[test]
fn open_hands_a_folder_to_a_window() {
    let scratch = Scratch::new();
    let service = service(&scratch, true);
    let launcher = Launcher::new(&service.bus);
    let folder = scratch.home().join("Documents");
    launcher.call("Open", &folder, Some(TOKEN)).unwrap();
    assert_eq!(service.windows.recv_timeout(WAIT), Ok(Some(folder)));
}

#[test]
fn show_in_folder_opens_the_holding_folder() {
    let scratch = Scratch::new();
    let service = service(&scratch, true);
    let launcher = Launcher::new(&service.bus);
    launcher
        .call("ShowInFolder", &scratch.file(), None)
        .unwrap();
    let folder = scratch.home().join("Documents");
    assert_eq!(service.windows.recv_timeout(WAIT), Ok(Some(folder)));
    assert!(launcher.call("ShowInFolder", Path::new("/"), None).is_err());
}

/// Opening a window tells the core first, so it doesn't stop while the
/// window starts.
fn opening_a_window_keeps_the_core_up(method: &str, path: fn(&Scratch) -> PathBuf) {
    let scratch = Scratch::new();
    let service = service_with(&scratch, false, SHORT_IDLE);
    let launcher = Launcher::new(&service.bus);
    assert!(
        service
            .core_ended
            .recv_timeout(SHORT_IDLE * 6 / 10)
            .is_err()
    );
    launcher.call(method, &path(&scratch), None).unwrap();
    service.windows.recv_timeout(WAIT).unwrap();
    // Past the first idle exit, short of the one from the call.
    assert!(
        service
            .core_ended
            .recv_timeout(SHORT_IDLE * 7 / 10)
            .is_err()
    );
}

#[test]
fn open_on_a_folder_keeps_the_core_up() {
    opening_a_window_keeps_the_core_up("Open", Scratch::home);
}

#[test]
fn show_in_folder_keeps_the_core_up() {
    opening_a_window_keeps_the_core_up("ShowInFolder", Scratch::file);
}

#[test]
fn paths_must_be_absolute() {
    let scratch = Scratch::new();
    let service = service(&scratch, true);
    let launcher = Launcher::new(&service.bus);
    for method in ["Open", "ShowInFolder"] {
        assert!(launcher.call(method, Path::new("Documents"), None).is_err());
    }
    assert!(service.windows.recv_timeout(QUIET).is_err());
}

/// What the stand-in portal was asked to open: the file's inode and the
/// activation token.
type Opened = (u64, Option<String>);

struct Portal {
    opened: Sender<Opened>,
}

#[zbus::interface(name = "org.freedesktop.portal.OpenURI")]
impl Portal {
    fn open_file(
        &self,
        parent_window: &str,
        fd: zbus::zvariant::OwnedFd,
        options: HashMap<String, OwnedValue>,
    ) -> OwnedObjectPath {
        assert_eq!(parent_window, "");
        let inode = File::from(std::os::fd::OwnedFd::from(fd))
            .metadata()
            .unwrap()
            .ino();
        let token = options
            .into_iter()
            .find(|(key, _)| key == "activation_token")
            .map(|(_, token)| String::try_from(token).unwrap());
        self.opened.send((inode, token)).ok();
        OwnedObjectPath::try_from("/org/freedesktop/portal/desktop/request/1").unwrap()
    }
}

fn portal(bus: &PrivateBus) -> (Connection, Receiver<Opened>) {
    let (opened, received) = mpsc::channel();
    let connection = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .serve_at(PORTAL_PATH, Portal { opened })
        .unwrap()
        .name(PORTAL_NAME)
        .unwrap()
        .build()
        .unwrap();
    (connection, received)
}

#[test]
fn open_hands_a_file_to_the_portal_with_the_token() {
    let scratch = Scratch::new();
    let service = service(&scratch, true);
    let (_portal, opened) = portal(&service.bus);
    let launcher = Launcher::new(&service.bus);
    let inode = fs::metadata(scratch.untyped()).unwrap().ino();
    launcher
        .call("Open", &scratch.untyped(), Some(TOKEN))
        .unwrap();
    assert_eq!(
        opened.recv_timeout(WAIT),
        Ok((inode, Some(TOKEN.to_owned())))
    );
    launcher.call("Open", &scratch.untyped(), None).unwrap();
    assert_eq!(opened.recv_timeout(WAIT), Ok((inode, None)));
    assert!(service.windows.recv_timeout(QUIET).is_err());
}

#[test]
fn open_never_waits_on_what_it_opens() {
    let scratch = Scratch::new();
    let service = service(&scratch, true);
    let (_portal, opened) = portal(&service.bus);
    let launcher = Launcher::new(&service.bus);
    // Opening a FIFO to read waits for a writer; a path descriptor doesn't.
    let fifo = scratch.home().join("pipe");
    fifo_at(&fifo);
    let inode = fs::metadata(&fifo).unwrap().ino();
    launcher.call("Open", &fifo, None).unwrap();
    assert_eq!(opened.recv_timeout(WAIT), Ok((inode, None)));
}

#[test]
fn open_fails_for_a_missing_file_or_without_a_portal() {
    let scratch = Scratch::new();
    let service = service(&scratch, true);
    let launcher = Launcher::new(&service.bus);
    assert!(launcher.call("Open", &scratch.untyped(), None).is_err());
    let (_portal, opened) = portal(&service.bus);
    let missing = scratch.home().join("missing.pdf");
    assert!(launcher.call("Open", &missing, None).is_err());
    assert!(opened.recv_timeout(QUIET).is_err());
}

/// Set in the run with apps of its own: their folder.
const APPS_VAR: &str = "NOVA_FILES_DBUS_TEST_APPS";
const OPENS_IN_THE_APP: &str = "open_starts_the_default_app_without_the_portal";

/// The core reads the default apps from the environment, so the test runs
/// itself again with the environment pointed at apps of its own.
#[test]
fn open_starts_the_default_app_without_the_portal() {
    match std::env::var_os(APPS_VAR) {
        Some(apps) => open_in_the_default_app(Path::new(&apps)),
        None => run_again_with_apps_of_its_own(),
    }
}

fn run_again_with_apps_of_its_own() {
    let root = std::env::temp_dir().join(format!("nova-files-dbus-apps-{}", std::process::id()));
    let data = root.join("data");
    let config = root.join("config");
    fs::create_dir_all(data.join("mime")).unwrap();
    fs::create_dir_all(data.join("applications")).unwrap();
    fs::create_dir_all(&config).unwrap();
    fs::write(data.join("mime/globs2"), "50:application/pdf:*.pdf\n").unwrap();
    fs::write(
        config.join("mimeapps.list"),
        "[Default Applications]\napplication/pdf=nova-test-viewer.desktop\n",
    )
    .unwrap();
    let viewer = root.join("viewer");
    let script = format!(
        "#!/bin/sh\nprintf '%s %s' \"$1\" \"$XDG_ACTIVATION_TOKEN\" > '{}'\n",
        root.join("out").display()
    );
    fs::write(&viewer, script).unwrap();
    fs::set_permissions(&viewer, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Viewer\nExec={} %f\n",
        viewer.display()
    );
    fs::write(data.join("applications/nova-test-viewer.desktop"), entry).unwrap();
    // Reading a FIFO waits for the viewer to write it, with no polling.
    fifo_at(&root.join("out"));
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", OPENS_IN_THE_APP, "--test-threads=1"])
        .env(APPS_VAR, &root)
        .env("HOME", &root)
        .env("XDG_DATA_HOME", &data)
        .env("XDG_DATA_DIRS", &data)
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_CONFIG_DIRS", &config)
        .env_remove("XDG_CURRENT_DESKTOP")
        .status()
        .unwrap();
    fs::remove_dir_all(&root).unwrap();
    assert!(status.success());
}

fn open_in_the_default_app(apps: &Path) {
    let scratch = Scratch::new();
    let service = service(&scratch, true);
    let (_portal, opened) = portal(&service.bus);
    let launcher = Launcher::new(&service.bus);
    launcher.call("Open", &scratch.file(), Some(TOKEN)).unwrap();
    let written = fs::read_to_string(apps.join("out")).unwrap();
    assert_eq!(written, format!("{} {TOKEN}", scratch.file().display()));
    assert!(opened.recv_timeout(QUIET).is_err());
}
