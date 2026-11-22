use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use nova_files_api::{ClientGone, ClientId, Envelope, EventSink};

use super::*;
use crate::set_aside::{Aside, SetAsideForJob};

const WAIT: Duration = Duration::from_secs(10);
const PARTWAY: &str = "/gone/partway";

#[derive(Debug, PartialEq, Eq)]
struct Made;

struct Undoing;

impl JobKind for Undoing {
    type Op = Operation;
    type Undo = ();

    fn run(&self, _: &mut Running, operation: &Operation, _: Option<&()>) -> Ended {
        match operation {
            Operation::Undo { .. } => (
                Outcome::Failed {
                    path: PathBuf::from(PARTWAY),
                    reason: Failure::NotFound,
                },
                Vec::new(),
            ),
            Operation::Feature(_) => (Outcome::Done, Vec::new()),
        }
    }

    fn undo(&self, _: &mut Running, operation: &Operation, _: &Ended) -> Option<()> {
        operation.get::<Made>().map(|_| ())
    }

    fn undoes(&self, operation: &Operation) -> Option<JobId> {
        match operation {
            Operation::Undo { job } => Some(*job),
            Operation::Feature(_) => None,
        }
    }

    fn first_path(&self, _: &Operation) -> PathBuf {
        PathBuf::new()
    }
}

struct Nowhere;

impl SetAside for Nowhere {
    fn for_job(&self) -> Box<dyn SetAsideForJob> {
        Box::new(Self)
    }
}

impl SetAsideForJob for Nowhere {
    fn set_aside(&mut self, _: &mut Running, _: (usize, &Path), _: &Path) -> Aside {
        Aside::Nowhere
    }

    fn trash(&mut self, _: &mut Running, _: &[PathBuf]) -> Ended {
        (Outcome::Done, Vec::new())
    }

    fn put_back(&mut self, _: &mut Running, _: &[PathBuf]) -> Option<Ended> {
        None
    }
}

struct Heard(Sender<Envelope>);

impl EventSink for Heard {
    fn deliver_envelope(&self, envelope: Envelope) -> Result<(), ClientGone> {
        self.0.send(envelope).map_err(|_| ClientGone)
    }
}

fn next(heard: &Receiver<Envelope>) -> Event {
    heard.recv_timeout(WAIT).unwrap().open::<Event>().unwrap()
}

fn until_finished(heard: &Receiver<Envelope>, job: JobId) -> Vec<Event> {
    let mut events = Vec::new();
    loop {
        let event = next(heard);
        let finished = matches!(event, Event::JobFinished { job: ended, .. } if ended == job);
        events.push(event);
        if finished {
            return events;
        }
    }
}

fn started(heard: &Receiver<Envelope>) -> JobId {
    loop {
        if let Event::JobStarted { job, .. } = next(heard) {
            return job;
        }
    }
}

#[test]
fn ids_start_from_the_time_so_no_run_repeats_another() {
    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros();

    let first = u128::from(first_id());

    assert!(first >= before, "{first} is before {before}");
}

#[test]
fn an_undo_that_stops_partway_on_a_missing_file_can_be_undone_again() {
    let (wake, _woken) = nova_files_api::channel();
    let jobs = Jobs::new(None, Arc::new(Undoing), Arc::new(Nowhere), wake);
    let (sender, heard) = mpsc::channel();
    let sink: Arc<dyn EventSink> = Arc::new(Heard(sender));
    jobs.watch(ClientId(1), &sink);
    let made = Operation::feature(Made);
    jobs.start(sink.as_ref(), JobTag(1), made).unwrap();
    let job = started(&heard);
    until_finished(&heard, job);

    jobs.start(sink.as_ref(), JobTag(2), Operation::Undo { job })
        .unwrap();
    let undo = started(&heard);
    let events = until_finished(&heard, undo);

    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Undoable { job: kept, .. } if *kept == job)),
        "the undo left history: {events:?}"
    );
}
