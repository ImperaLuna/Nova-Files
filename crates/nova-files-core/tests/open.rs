//! The core opens a file in the default app for its type, read from the
//! MIME database, `mimeapps.list` and the app's desktop entry, and says so;
//! a file it can't open is left to the client.
//!
//! The core reads those from the environment, so the test runs itself
//! again with the environment pointed at folders of its own.

#![expect(clippy::unwrap_used, reason = "test setup should fail loudly")]

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::sync::mpsc;
use std::time::Duration;

#[path = "common/own_folders.rs"]
mod own_folders;

use nova_files_api::{Command, Event};

use own_folders::run_again_in;

/// Set in the run that talks to the core: the folder it was given.
const ROOT_VAR: &str = "NOVA_FILES_OPEN_TEST_ROOT";
const TEST_NAME: &str = "a_file_opens_in_its_default_app";
const IDLE_EXIT: Duration = Duration::from_secs(5);
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn a_file_opens_in_its_default_app() {
    match std::env::var_os(ROOT_VAR) {
        Some(root) => open_through_the_core(Path::new(&root)),
        None => run_again_in_folders_of_its_own(),
    }
}

fn run_again_in_folders_of_its_own() {
    let root = std::env::temp_dir().join(format!("nova-files-open-{}", std::process::id()));
    fs::create_dir_all(root.join("data/mime")).unwrap();
    fs::create_dir_all(root.join("data/applications")).unwrap();
    fs::create_dir_all(root.join("config")).unwrap();
    // Only the parent type has a default app, so the core must look past
    // the file's own type.
    write(
        &root,
        "data/mime/globs2",
        "50:text/x-nova-test:*.novatest\n",
    );
    write(
        &root,
        "data/mime/subclasses",
        "text/x-nova-test text/x-nova-parent\n",
    );
    write(
        &root,
        "config/mimeapps.list",
        "[Default Applications]\ntext/x-nova-parent=nova-test-viewer.desktop\n",
    );
    let viewer = root.join("viewer");
    write(
        &root,
        "viewer",
        &format!(
            "#!/bin/sh\nprintf '%s %s' \"$1\" \"$XDG_ACTIVATION_TOKEN\" > '{}'\n",
            root.join("out").display()
        ),
    );
    fs::set_permissions(&viewer, fs::Permissions::from_mode(0o755)).unwrap();
    write(
        &root,
        "data/applications/nova-test-viewer.desktop",
        &format!(
            "[Desktop Entry]\nType=Application\nName=Viewer\nExec={} %f\n",
            viewer.display()
        ),
    );
    // Reading a FIFO waits for the viewer to write it, with no polling.
    let made = Process::new("mkfifo")
        .arg(root.join("out"))
        .status()
        .unwrap();
    assert!(made.success());

    run_again_in(&root, ROOT_VAR, TEST_NAME);
}

fn open_through_the_core(root: &Path) {
    let core = nova_files_core::spawn_with_idle_exit(IDLE_EXIT).unwrap();
    let (events, received) = mpsc::channel();
    let connection = core.connector.connect(events).unwrap();
    let open = |path: PathBuf| {
        connection
            .send(Command::OpenFile {
                path,
                activation: Some("tok".to_owned()),
            })
            .unwrap();
        received.recv_timeout(ANSWER_TIMEOUT).unwrap()
    };

    // No type from its name, so no app.
    assert_eq!(open(root.join("plain")), Event::FileNotOpened);
    let file = root.join("a.novatest");
    assert_eq!(open(file.clone()), Event::FileOpened);
    // On a thread, so a viewer that never runs fails the test, not hangs it.
    let out = root.join("out");
    let (sender, written) = mpsc::channel();
    std::thread::spawn(move || sender.send(fs::read_to_string(out).unwrap()));
    let written = written.recv_timeout(ANSWER_TIMEOUT).unwrap();
    assert_eq!(written, format!("{} tok", file.display()));
}

fn write(root: &Path, file: &str, text: &str) {
    fs::write(root.join(file), text).unwrap();
}
