//! A core with the real helper, in a scratch folder, and a client that
//! starts jobs and follows them.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use nova_files_api::{
    Command, Conflict, Connection, Event, JobId, JobTag, Operation, Outcome, Tally,
};
use nova_files_core::{Archiver, Home, IDLE_EXIT, RunningCore};

const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

/// A scratch folder with a core whose home is inside it, and a client.
pub struct Bench {
    root: PathBuf,
    _core: RunningCore,
    connection: Connection,
    events: Receiver<Event>,
}

impl Bench {
    pub fn new(test: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("nova-archiver-job-{test}-{}", std::process::id()));
        if root.exists() {
            fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).ok();
            fs::remove_dir_all(&root).ok();
        }
        fs::create_dir_all(root.join("home")).unwrap();
        fs::create_dir_all(root.join("into")).unwrap();
        let home = Home {
            folder: root.join("home"),
            config: root.join("home/.config"),
            state: root.join("home/.local/state"),
            data: root.join("home/.local/share"),
            drive_trashes: false,
        };
        let helper = PathBuf::from(env!("CARGO_BIN_EXE_nova-files-archiver"));
        let core = nova_files_core::spawn_with_archiver(
            IDLE_EXIT,
            Some(home),
            None,
            None,
            Some(Archiver::at(helper).with_seven(nova_files_core::seven_on(
                std::env::var_os("PATH").as_deref(),
            ))),
        )
        .unwrap();
        let (sink, events) = mpsc::channel();
        let connection = core.connector.connect(sink).unwrap();
        connection.send(Command::WatchJobs).unwrap();
        Self {
            root,
            _core: core,
            connection,
            events,
        }
    }

    pub fn at(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    pub fn start(&self, operation: Operation) -> JobId {
        let tag = JobTag(1);
        self.send(Command::StartJob { tag, operation });
        loop {
            if let Event::JobStarted { tag: got, job, .. } = self.next()
                && got == tag
            {
                return job;
            }
        }
    }

    pub fn send(&self, command: Command) {
        self.connection.send(command).unwrap();
    }

    pub fn next(&self) -> Event {
        self.events.recv_timeout(ANSWER_TIMEOUT).unwrap()
    }

    /// The conflict job `job` holds at next.
    pub fn held(&self, job: JobId) -> Conflict {
        loop {
            match self.next() {
                Event::JobConflict { job: at, conflict } if at == job => return conflict,
                Event::JobFinished { outcome, .. } => panic!("ended {outcome:?} unasked"),
                _ => {}
            }
        }
    }

    /// How job `job` ended, and what answers made of it.
    pub fn finished(&self, job: JobId) -> (Outcome, Tally) {
        loop {
            match self.next() {
                Event::JobFinished {
                    job: ended,
                    outcome,
                    tally,
                    ..
                } if ended == job => return (outcome, tally),
                Event::JobConflict { conflict, .. } => panic!("held at {conflict:?}"),
                _ => {}
            }
        }
    }

    /// What `into` holds, by name, sorted.
    pub fn landed(&self) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(self.at("into"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).ok();
    }
}
