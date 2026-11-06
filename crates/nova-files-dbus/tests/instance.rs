//! Against a private D-Bus daemon: the first start serves `Ping` through
//! the real core, and a second start finds it and hands over a window
//! request through `org.freedesktop.Application.Activate`.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test setup should fail loudly"
)]

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use nova_files_dbus::{APP_ID, Bus, FILES_INTERFACE, Started};
use zbus::blocking::Proxy;

/// How long a test waits for the bus or a call before it counts as failed.
const WAIT: Duration = Duration::from_secs(10);

/// A minimal bus config: a socket in the test's own folder, anyone on it
/// may own any name. It doesn't rely on the system's session config, which
/// the Nix build sandbox doesn't have.
const BUS_CONFIG: &str = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir=@DIR@</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#;

/// A dbus-daemon of the test's own, stopped when dropped.
struct PrivateBus {
    daemon: Child,
    address: String,
    _folder: TempFolder,
}

/// A folder under the system temp folder, removed when dropped.
struct TempFolder(PathBuf);

impl TempFolder {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "nova-files-dbus-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

impl PrivateBus {
    fn start() -> Self {
        let folder = TempFolder::new();
        let config = folder.0.join("bus.conf");
        fs::write(
            &config,
            BUS_CONFIG.replace("@DIR@", &folder.0.to_string_lossy()),
        )
        .unwrap();
        let mut daemon = Command::new("dbus-daemon")
            .arg("--nofork")
            .arg("--print-address=1")
            .arg(format!("--config-file={}", config.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("dbus-daemon is on PATH");
        let stdout = daemon.stdout.take().unwrap();
        let (line_sender, line) = mpsc::channel();
        thread::spawn(move || {
            let mut lines = BufReader::new(stdout).lines();
            if let Some(Ok(address)) = lines.next() {
                line_sender.send(address).ok();
            }
            // Drain the rest so the daemon never blocks on a full pipe.
            lines.for_each(drop);
        });
        let address = line
            .recv_timeout(WAIT)
            .expect("dbus-daemon printed its address");
        Self {
            daemon,
            address,
            _folder: folder,
        }
    }

    fn bus(&self) -> Bus {
        Bus::Address(self.address.clone())
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        self.daemon.kill().ok();
        self.daemon.wait().ok();
    }
}

#[test]
fn first_start_answers_ping_through_the_core() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn().unwrap();
    let started = nova_files_dbus::start(bus.bus(), core.connector, || {}).unwrap();
    assert!(matches!(started, Started::Service(_)));

    let client = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .method_timeout(WAIT)
        .build()
        .unwrap();
    let files = Proxy::new(
        &client,
        APP_ID,
        nova_files_dbus::object_path().unwrap(),
        FILES_INTERFACE,
    )
    .unwrap();

    files.call::<_, _, ()>("Ping", &()).unwrap();
}

#[test]
fn second_start_hands_its_window_to_the_first() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn().unwrap();
    let (activations, activated) = mpsc::channel();
    let first = nova_files_dbus::start(bus.bus(), core.connector.clone(), move || {
        activations.send(()).ok();
    })
    .unwrap();
    assert!(matches!(first, Started::Service(_)));

    let second = nova_files_dbus::start(bus.bus(), core.connector, || {}).unwrap();
    let Started::AlreadyRunning(running) = second else {
        panic!("the second start should find the first");
    };
    running.activate().unwrap();

    assert_eq!(activated.recv_timeout(WAIT), Ok(()));
}

#[test]
fn name_is_free_again_after_the_service_drops() {
    let bus = PrivateBus::start();
    let core = nova_files_core::spawn().unwrap();
    let first = nova_files_dbus::start(bus.bus(), core.connector.clone(), || {}).unwrap();
    drop(first);

    let again = nova_files_dbus::start(bus.bus(), core.connector, || {}).unwrap();
    assert!(matches!(again, Started::Service(_)));
}
