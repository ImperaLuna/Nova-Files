//! What the job tests share: a scratch folder with a core whose home is
//! inside it, and a client that starts jobs and waits for them.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use nova_files_api::{Command, Connection, Event, JobId, JobTag, Operation, Outcome};

/// How long a test waits for the core before it counts as a failure.
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

/// A scratch folder for one test, and a core whose home is `home` in it.
pub struct Bench {
    pub root: PathBuf,
    pub core: nova_files_core::RunningCore,
}

impl Bench {
    pub fn new(test: &str) -> Self {
        Self::with(test, nova_files_core::IDLE_EXIT, |_| {})
    }

    /// A bench whose core exits after `idle_exit` idle, started once
    /// `prepare` has had the fresh scratch folder.
    pub fn with(test: &str, idle_exit: Duration, prepare: impl FnOnce(&Path)) -> Self {
        let root =
            std::env::temp_dir().join(format!("nova-files-jobs-{test}-{}", std::process::id()));
        fs::remove_dir_all(&root).ok();
        fs::create_dir_all(root.join("home")).unwrap();
        prepare(&root);
        let home = nova_files_core::Home {
            folder: root.join("home"),
            config: root.join("home/.config"),
            state: root.join("home/.local/state"),
        };
        let core = nova_files_core::spawn_with(idle_exit, Some(home), None, None).unwrap();
        Self { root, core }
    }

    pub fn at(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    pub fn client(&self) -> Client {
        let (sink, events) = mpsc::channel();
        let connection = self.core.connector.connect(sink).unwrap();
        Client { connection, events }
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).ok();
    }
}

pub struct Client {
    pub connection: Connection,
    pub events: Receiver<Event>,
}

impl Client {
    /// Starts `operation` and returns its id once the core names it.
    pub fn start(&self, operation: Operation) -> JobId {
        let tag = JobTag(7);
        self.connection
            .send(Command::StartJob { tag, operation })
            .unwrap();
        match self.next() {
            Event::JobStarted { tag: got, job } if got == tag => job,
            other => panic!("expected the job's id first, got {other:?}"),
        }
    }

    /// The next event, failing the test after [`ANSWER_TIMEOUT`].
    pub fn next(&self) -> Event {
        self.events.recv_timeout(ANSWER_TIMEOUT).unwrap()
    }

    /// Waits for job `job` to end, skipping its progress, and returns how
    /// it ended and the folders it changed.
    pub fn ended(&self, job: JobId) -> (Outcome, Vec<PathBuf>) {
        loop {
            match self.next() {
                Event::JobFinished {
                    job: ended,
                    outcome,
                    changed,
                } if ended == job => return (outcome, changed),
                Event::JobProgress { .. } => {}
                other => panic!("expected job {job:?} to end, got {other:?}"),
            }
        }
    }

    /// Runs `operation` to its end.
    pub fn run(&self, operation: Operation) -> (JobId, Outcome, Vec<PathBuf>) {
        let job = self.start(operation);
        let (outcome, changed) = self.ended(job);
        (job, outcome, changed)
    }

    pub fn send(&self, command: Command) {
        self.connection.send(command).unwrap();
    }
}
