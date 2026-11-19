use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use nova_files_api::{Command, Event};

const IDLE_EXIT: Duration = Duration::from_millis(200);
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn ping_gets_pong() {
    let core = nova_files::spawn_with_idle_exit(IDLE_EXIT).unwrap();
    let (events, received) = mpsc::channel();
    let connection = core.connector.connect(events).unwrap();

    connection.send(Command::Ping).unwrap();

    assert_eq!(received.recv_timeout(ANSWER_TIMEOUT), Ok(Event::Pong));
}

#[test]
fn stays_up_while_a_client_is_connected() {
    let core = nova_files::spawn_with_idle_exit(IDLE_EXIT).unwrap();
    let (events, received) = mpsc::channel();
    let connection = core.connector.connect(events).unwrap();

    assert_eq!(
        received.recv_timeout(IDLE_EXIT * 3),
        Err(mpsc::RecvTimeoutError::Timeout)
    );
    connection.send(Command::Ping).unwrap();

    assert_eq!(received.recv_timeout(ANSWER_TIMEOUT), Ok(Event::Pong));
    assert!(!core.thread.is_finished());
}

#[test]
fn stops_when_idle_after_the_last_client_leaves() {
    let core = nova_files::spawn_with_idle_exit(IDLE_EXIT).unwrap();
    let (events, _received) = mpsc::channel::<Event>();
    let connection = core.connector.connect(events).unwrap();

    drop(connection);
    let (stopped, joined) = mpsc::channel();
    let core_thread = core.thread;
    thread::spawn(move || stopped.send(core_thread.join().is_ok()));
    assert_eq!(joined.recv_timeout(ANSWER_TIMEOUT), Ok(true));

    let (events, _received) = mpsc::channel::<Event>();
    assert!(core.connector.connect(events).is_err());
}

#[test]
fn activity_restarts_the_idle_wait() {
    let core = nova_files::spawn_with_idle_exit(IDLE_EXIT).unwrap();
    let (_never_sent, quiet) = mpsc::channel::<()>();

    for _ in 0..4 {
        assert_eq!(
            quiet.recv_timeout(IDLE_EXIT / 2),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        core.connector.activity().unwrap();
    }

    assert!(!core.thread.is_finished());
}
