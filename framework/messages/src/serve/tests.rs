use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;

use super::*;
use crate::connection::{ClientGone, channel};

#[derive(Debug, PartialEq, Eq)]
struct Echo(u32);

#[derive(Default)]
struct State {
    busy: AtomicBool,
    offers: Mutex<Vec<u32>>,
    left: Mutex<Vec<ClientId>>,
    checked: Option<mpsc::Sender<()>>,
}

impl Served for State {
    type Client = Arc<dyn EventSink>;

    fn client(&self, events: Arc<dyn EventSink>) -> Self::Client {
        events
    }

    fn busy(&self) -> bool {
        if let Some(checked) = &self.checked {
            checked.send(()).ok();
        }
        self.busy.load(Ordering::SeqCst)
    }

    fn offered(&self, offer: Envelope) {
        if let Ok(number) = offer.open::<u32>() {
            self.offers.lock().unwrap().push(number);
        }
    }
}

fn routes() -> Routes<State, Arc<dyn EventSink>> {
    let mut routes = Routes::new();
    routes
        .on(
            |_: &State, _, client: &Arc<dyn EventSink>, Echo(n)| -> Result<(), ClientGone> {
                client.deliver(Echo(n + 1))
            },
        )
        .unwrap();
    routes.on_leave(|state: &State, client| state.left.lock().unwrap().push(client));
    routes
}

const LONG: Duration = Duration::from_hours(1);

#[test]
fn an_idle_core_with_no_clients_returns_after_the_idle_exit() {
    let (_connector, core) = channel();
    serve(
        &core,
        Duration::from_millis(5),
        &routes(),
        &State::default(),
    );
}

#[test]
fn a_busy_core_waits_past_the_idle_exit() {
    let (connector, core) = channel();
    let (checked, checks) = mpsc::channel();
    let state = State {
        busy: AtomicBool::new(true),
        checked: Some(checked),
        ..State::default()
    };
    let served = thread::spawn(move || {
        serve(&core, Duration::from_millis(1), &routes(), &state);
        state
    });
    assert_eq!(checks.recv(), Ok(()));
    assert!(checks.recv_timeout(Duration::from_millis(30)).is_err());
    assert!(!served.is_finished());
    drop(connector);
    assert!(served.join().unwrap().busy());
}

#[test]
fn a_stopped_core_answers_its_clients_and_returns_when_the_last_one_leaves() {
    let (connector, core) = channel();
    let served = thread::spawn(move || {
        let state = State::default();
        serve(&core, LONG, &routes(), &state);
        state
    });
    let (events, answers) = mpsc::channel::<Echo>();
    let connection = connector.connect(events).unwrap();
    connector.stop().unwrap();
    connection.send(Echo(1)).unwrap();
    assert_eq!(answers.recv(), Ok(Echo(2)));
    connector.offer(7_u32).unwrap();
    connection.send(3_u8).unwrap();
    drop(connection);
    let state = served.join().unwrap();
    assert_eq!(*state.offers.lock().unwrap(), vec![7]);
    assert_eq!(*state.left.lock().unwrap(), vec![ClientId(0)]);
}

#[test]
fn a_client_whose_events_are_gone_is_forgotten() {
    let (connector, core) = channel();
    let served = thread::spawn(move || {
        let state = State::default();
        serve(&core, LONG, &routes(), &state);
        state
    });
    let (events, answers) = mpsc::channel::<Echo>();
    let connection = connector.connect(events).unwrap();
    drop(answers);
    connector.stop().unwrap();
    connection.send(Echo(1)).unwrap();
    let state = served.join().unwrap();
    assert_eq!(*state.left.lock().unwrap(), vec![ClientId(0)]);
    drop(connection);
}
