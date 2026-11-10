use std::fs;
use std::sync::mpsc::{self, Receiver};

use super::*;
use crate::ops::test_folder::TestFolder;

/// Folders in a tree big enough that a scan still runs when it is cut.
const MANY: usize = 2000;
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

fn client() -> (Arc<dyn EventSink>, Receiver<Event>) {
    let (sink, events) = mpsc::channel();
    (Arc::new(sink), events)
}

/// The state of scan `id`'s last answer.
fn last_state(events: &Receiver<Event>, id: MeasureId) -> ScanState {
    loop {
        match events.recv_timeout(ANSWER_TIMEOUT).unwrap() {
            Event::SizeMeasured { id: got, state, .. } if got == id => {
                if state != ScanState::Counting {
                    return state;
                }
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

fn big_tree(folder: &TestFolder) {
    for name in 0..MANY {
        fs::create_dir_all(folder.path().join(format!("{name}/inside"))).unwrap();
    }
}

#[test]
fn stopping_scans_under_a_folder_cuts_them_short_and_waits_for_them() {
    let folder = TestFolder::new("measure-stop-under");
    big_tree(&folder);
    let measurer = Measurer::default();
    let (sink, events) = client();
    let id = MeasureId(1);

    measurer.measure(ClientId(1), &sink, id, vec![folder.path().to_owned()]);
    let let_go = measurer.stop_under(folder.path());

    assert!(let_go);
    assert_eq!(last_state(&events, id), ScanState::CutShort);
    assert!(lock(&measurer.scans).is_empty());
}

#[test]
fn scans_elsewhere_go_on_when_another_folder_is_let_go() {
    let folder = TestFolder::new("measure-stop-elsewhere");
    let other = TestFolder::new("measure-stop-other");
    fs::write(folder.path().join("file"), [0; 4]).unwrap();
    let measurer = Measurer::default();
    let (sink, events) = client();
    let id = MeasureId(2);

    measurer.measure(ClientId(1), &sink, id, vec![folder.path().to_owned()]);
    assert!(measurer.stop_under(other.path()));

    assert_eq!(last_state(&events, id), ScanState::Done);
}

#[test]
fn a_stopped_scan_is_not_cut_short_later() {
    let folder = TestFolder::new("measure-stopped-then-cut");
    big_tree(&folder);
    let measurer = Measurer::default();
    let (sink, events) = client();
    let id = MeasureId(3);
    let client = ClientId(1);
    measurer.measure(client, &sink, id, vec![folder.path().to_owned()]);
    let shared = Arc::clone(lock(&measurer.scans).get(&client).unwrap());

    measurer.stop(client, id);
    measurer.stop_under(folder.path());

    assert!(shared.wait_ended(Instant::now() + ANSWER_TIMEOUT));
    while let Ok(event) = events.try_recv() {
        assert!(
            matches!(
                event,
                Event::SizeMeasured {
                    state: ScanState::Counting,
                    ..
                }
            ),
            "{event:?}"
        );
    }
}

#[test]
fn waiting_for_a_scan_that_never_ends_gives_up_at_the_deadline() {
    let shared = running(MeasureId(4));

    assert!(!shared.wait_ended(Instant::now() + EVERY));
    shared.end();
    assert!(shared.wait_ended(Instant::now()));
}

fn running(id: MeasureId) -> Arc<Shared> {
    Arc::new(Shared {
        id,
        roots: Vec::new(),
        state: AtomicU8::new(RUNNING),
        ended: Mutex::new(false),
        ended_signal: Condvar::new(),
    })
}

#[test]
fn a_scan_tells_its_client_only_while_it_runs() {
    let measurer = Measurer::default();
    let (sink, events) = client();
    let client = ClientId(1);
    let shared = running(MeasureId(5));
    lock(&measurer.scans).insert(client, Arc::clone(&shared));
    let scan = Scan {
        scans: Arc::clone(&measurer.scans),
        client,
        shared,
        events: sink,
    };

    scan.tell(Event::Pong).unwrap();
    let while_running: Vec<Event> = events.try_iter().collect();
    measurer.forget(client);
    scan.tell(Event::Pong).unwrap();
    let once_stopped: Vec<Event> = events.try_iter().collect();

    assert_eq!(while_running, vec![Event::Pong]);
    assert_eq!(once_stopped, Vec::new());
    assert!(!scan.shared.running());
}

#[test]
fn a_scan_reports_at_most_every_tenth_of_a_second() {
    let folder = TestFolder::new("measure-spacing");
    big_tree(&folder);
    let measurer = Measurer::default();
    let (sink, events) = client();
    let id = MeasureId(6);
    let started = Instant::now();

    measurer.measure(ClientId(1), &sink, id, vec![folder.path().to_owned()]);
    let mut reports: u128 = 0;
    loop {
        reports += 1;
        match events.recv_timeout(ANSWER_TIMEOUT).unwrap() {
            Event::SizeMeasured {
                state: ScanState::Counting,
                ..
            } => {}
            Event::SizeMeasured {
                state: ScanState::Done,
                ..
            } => break,
            other => panic!("unexpected {other:?}"),
        }
    }

    let most = started.elapsed().as_millis() / EVERY.as_millis() + 2;
    assert!(reports <= most, "{reports} reports, at most {most}");
}
