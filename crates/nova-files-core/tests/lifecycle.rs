//! The core answers its clients, stays up while one is connected and stops
//! once it has been idle.

use std::sync::mpsc;
use std::time::Duration;

use nova_files_api::{Command, Event};

/// Short enough to keep the tests fast, long enough that a test's next
/// message always arrives first.
const IDLE_EXIT: Duration = Duration::from_millis(200);
/// How long a test waits for the core before it counts as a failure.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn ping_gets_pong() {
    let core = nova_files_core::spawn_with_idle_exit(IDLE_EXIT).unwrap();
    let (events, received) = mpsc::channel();
    let connection = core.connector.connect(events).unwrap();

    connection.send(Command::Ping).unwrap();

    assert_eq!(received.recv_timeout(ANSWER_TIMEOUT), Ok(Event::Pong));
}

#[test]
fn stays_up_while_a_client_is_connected() {
    let core = nova_files_core::spawn_with_idle_exit(IDLE_EXIT).unwrap();
    let (events, received) = mpsc::channel();
    let connection = core.connector.connect(events).unwrap();

    // Silence for several idle periods, with the client still connected.
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
    let core = nova_files_core::spawn_with_idle_exit(IDLE_EXIT).unwrap();
    let (events, _received) = mpsc::channel();
    let connection = core.connector.connect(events).unwrap();

    drop(connection);
    core.thread.join().unwrap();

    let (events, _received) = mpsc::channel();
    assert!(core.connector.connect(events).is_err());
}
