//! Open With through the real core: the apps for some files, opening many
//! files in one app, and making an app the default, which the next list
//! shows and the user's `mimeapps.list` keeps.
//!
//! The core reads the MIME database and the app lists from the
//! environment, so the test runs itself again with every XDG folder
//! pointed into a temp folder of its own: the machine's real apps never
//! start.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup should fail loudly"
)]

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

#[path = "common/own_folders.rs"]
mod own_folders;

use nova_files_api::{Command, Connection, Event, OpenWith, OpenWithAnswer, OpenWithId};

use own_folders::run_again_in;

/// Set in the run that talks to the core: the folder it was given.
const ROOT_VAR: &str = "NOVA_FILES_OPEN_WITH_TEST_ROOT";
const TEST_NAME: &str = "open_with_lists_launches_and_sets_the_default";
const IDLE_EXIT: Duration = Duration::from_secs(5);
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);
const MIME: &str = "text/x-nova-test";
/// Another type of the same kind as [`MIME`].
const OTHER: &str = "text/x-nova-other";
const PARENT: &str = "text/x-nova-parent";
const EXECUTABLE: u32 = 0o755;

#[test]
fn open_with_lists_launches_and_sets_the_default() {
    if let Some(root) = std::env::var_os(ROOT_VAR) {
        through_the_core(Path::new(&root));
        return;
    }
    let root = std::env::temp_dir().join(format!("nova-files-open-with-{}", std::process::id()));
    fs::remove_dir_all(&root).ok();
    let files = [
        (
            "data/mime/globs2",
            format!("50:{MIME}:*.novatest\n50:{OTHER}:*.novaother\n"),
        ),
        (
            "data/mime/subclasses",
            format!("{MIME} {PARENT}\n{OTHER} {PARENT}\n"),
        ),
        (
            "config/mimeapps.list",
            format!("# kept\n[Default Applications]\n{MIME}=one.desktop\n"),
        ),
        (
            "data/applications/mimeinfo.cache",
            format!(
                "[MIME Cache]\n{MIME}=one.desktop;many.desktop;term.desktop;\n{PARENT}=many.desktop;\n"
            ),
        ),
        ("data/applications/one.desktop", entry("One", "%f", "")),
        (
            "data/applications/many.desktop",
            entry("Many", "%F", "Icon=many\n"),
        ),
        (
            "data/applications/term.desktop",
            entry("Term", "%f", "Terminal=true\n"),
        ),
    ];
    for (file, text) in files {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    // The app writes its arguments where the test reads them; a FIFO makes
    // the read wait for it, with no polling.
    let app = root.join("app");
    fs::write(
        &app,
        format!(
            "#!/bin/sh\nprintf '%s|' \"$@\" > '{}'\n",
            root.join("out").display()
        ),
    )
    .unwrap();
    fs::set_permissions(&app, fs::Permissions::from_mode(EXECUTABLE)).unwrap();
    assert!(
        Process::new("mkfifo")
            .arg(root.join("out"))
            .status()
            .unwrap()
            .success()
    );
    run_again_in(&root, ROOT_VAR, TEST_NAME);
}

/// A desktop entry running the test's app with `code` for the files.
fn entry(name: &str, code: &str, extra: &str) -> String {
    format!("[Desktop Entry]\nType=Application\nName={name}\nExec=APP {code}\n{extra}")
}

struct Client {
    connection: Connection,
    events: Receiver<Event>,
}

impl Client {
    fn send(&self, request: OpenWith) {
        self.connection.send(Command::OpenWith(request)).unwrap();
    }

    fn next(&self) -> OpenWithAnswer {
        match self.events.recv_timeout(ANSWER_TIMEOUT).unwrap() {
            Event::OpenWith(answer) => answer,
            other => panic!("expected an Open With answer, got {other:?}"),
        }
    }

    /// Opens `paths` in `app`; says whether it started.
    fn launch(&self, paths: &[PathBuf], app: &str) -> bool {
        let id = OpenWithId(5);
        self.send(OpenWith::Launch {
            id,
            paths: paths.to_vec(),
            app: app.to_owned(),
            activation: None,
        });
        match self.next() {
            OpenWithAnswer::Launched {
                id: answered,
                started,
            } if answered == id => started,
            other => panic!("expected the launch's answer, got {other:?}"),
        }
    }

    /// The apps for `paths` as (id, default) pairs, and the shared type.
    fn apps(&self, paths: &[PathBuf]) -> (Option<String>, Vec<(String, bool)>) {
        let id = OpenWithId(3);
        self.send(OpenWith::Apps {
            id,
            paths: paths.to_vec(),
        });
        match self.next() {
            OpenWithAnswer::Apps {
                id: answered,
                mime,
                apps,
            } if answered == id => (
                mime,
                apps.into_iter().map(|app| (app.id, app.default)).collect(),
            ),
            other => panic!("expected the apps, got {other:?}"),
        }
    }
}

fn through_the_core(root: &Path) {
    // The entries name the app by a placeholder, so they could be written
    // before the app's path was known.
    for id in ["one", "many", "term"] {
        let path = root.join(format!("data/applications/{id}.desktop"));
        let text = fs::read_to_string(&path).unwrap();
        let app = root.join("app");
        fs::write(&path, text.replace("APP", app.to_str().unwrap())).unwrap();
    }
    let core = nova_files_core::spawn_with_idle_exit(IDLE_EXIT).unwrap();
    let (sink, events) = mpsc::channel();
    let client = Client {
        connection: core.connector.connect(sink).unwrap(),
        events,
    };
    let a = root.join("a.novatest");
    let b = root.join("b c.novatest");

    // Terminal apps are left out; an untyped name has no apps.
    let (mime, apps) = client.apps(&[a.clone(), b.clone()]);
    assert_eq!(mime.as_deref(), Some(MIME));
    assert_eq!(
        apps,
        [
            ("one.desktop".to_owned(), true),
            ("many.desktop".to_owned(), false)
        ]
    );
    // Files of two types get the apps for the type both are kinds of, with
    // no type to set a default for and no default.
    assert_eq!(
        client.apps(&[a.clone(), root.join("x.novaother")]),
        (None, vec![("many.desktop".to_owned(), false)])
    );
    assert_eq!(
        client.apps(&[a.clone(), root.join("plain")]),
        (None, vec![])
    );

    // `%F` takes both files in one run.
    assert!(client.launch(&[a.clone(), b.clone()], "many.desktop"));
    let out = root.join("out");
    let (sender, written) = mpsc::channel();
    std::thread::spawn(move || sender.send(fs::read_to_string(out).unwrap()));
    assert_eq!(
        written.recv_timeout(ANSWER_TIMEOUT).unwrap(),
        format!("{}|{}|", a.display(), b.display())
    );
    for refused in ["term.desktop", "missing.desktop", "../x.desktop"] {
        assert!(
            !client.launch(std::slice::from_ref(&a), refused),
            "{refused}"
        );
    }
    assert!(!client.launch(&[PathBuf::from("relative.novatest")], "one.desktop"));

    // Setting the default answers nothing; the next list shows it.
    client.send(OpenWith::SetDefault {
        mime: MIME.to_owned(),
        app: "many.desktop".to_owned(),
    });
    let (_, apps) = client.apps(&[a]);
    assert_eq!(
        apps,
        [
            ("many.desktop".to_owned(), true),
            ("one.desktop".to_owned(), false)
        ]
    );
    let list = fs::read_to_string(root.join("config/mimeapps.list")).unwrap();
    assert_eq!(
        list,
        format!(
            "# kept\n[Default Applications]\n{MIME}=many.desktop;\n\n[Added Associations]\n{MIME}=many.desktop;\n"
        )
    );
}
