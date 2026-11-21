use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use nova_files_api::{Command, Connection, Event, JobId, JobTag, Operation, Outcome, Tally};

pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Core(pub nova_files::RunningCore);

impl Deref for Core {
    type Target = nova_files::RunningCore;

    fn deref(&self) -> &nova_files::RunningCore {
        &self.0
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        self.0.connector.stop().ok();
    }
}

pub struct Bench {
    pub root: PathBuf,
    pub core: Core,
}

impl Bench {
    pub fn new(test: &str) -> Self {
        Self::with(test, nova_files::IDLE_EXIT, |_| {})
    }

    pub fn with(test: &str, idle_exit: Duration, prepare: impl FnOnce(&Path)) -> Self {
        Self::spawned(test, idle_exit, prepare, None)
    }

    pub fn with_archiver(test: &str, archiver: nova_files::Archiver) -> Self {
        Self::spawned(test, nova_files::IDLE_EXIT, |_| {}, Some(archiver))
    }

    fn spawned(
        test: &str,
        idle_exit: Duration,
        prepare: impl FnOnce(&Path),
        archiver: Option<nova_files::Archiver>,
    ) -> Self {
        let root =
            std::env::temp_dir().join(format!("nova-files-jobs-{test}-{}", std::process::id()));
        fs::remove_dir_all(&root).ok();
        fs::create_dir_all(root.join("home")).unwrap();
        prepare(&root);
        let home = nova_files::Home {
            folder: root.join("home"),
            config: root.join("home/.config"),
            state: root.join("home/.local/state"),
            data: root.join("home/.local/share"),
            drive_trashes: false,
        };
        let core = Core(
            nova_files::spawn_with_archiver(idle_exit, Some(home), None, None, archiver).unwrap(),
        );
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
    pub fn start(&self, operation: Operation) -> JobId {
        let tag = JobTag(7);
        self.connection
            .send(Command::StartJob { tag, operation })
            .unwrap();
        match self.next() {
            Event::JobStarted { tag: got, job, .. } if got == tag => job,
            other => panic!("expected the job's id first, got {other:?}"),
        }
    }

    pub fn next(&self) -> Event {
        loop {
            match self.any_next() {
                Event::Undoable { .. } | Event::UndoGone(_) => {}
                event => return event,
            }
        }
    }

    pub fn any_next(&self) -> Event {
        self.events.recv_timeout(ANSWER_TIMEOUT).unwrap()
    }

    pub fn ended(&self, job: JobId) -> (Outcome, Vec<PathBuf>) {
        let (outcome, changed, _) = self.finished(job);
        (outcome, changed)
    }

    pub fn finished(&self, job: JobId) -> (Outcome, Vec<PathBuf>, Tally) {
        loop {
            match self.next() {
                Event::JobFinished {
                    job: ended,
                    outcome,
                    changed,
                    tally,
                } if ended == job => return (outcome, changed, tally),
                Event::JobProgress { .. } => {}
                other => panic!("expected job {job:?} to end, got {other:?}"),
            }
        }
    }

    pub fn run(&self, operation: Operation) -> (JobId, Outcome, Vec<PathBuf>) {
        let job = self.start(operation);
        let (outcome, changed) = self.ended(job);
        (job, outcome, changed)
    }

    pub fn send(&self, command: Command) {
        self.connection.send(command).unwrap();
    }
}
