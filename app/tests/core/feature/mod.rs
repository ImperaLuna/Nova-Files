use std::sync::mpsc::{self, Receiver};

use nova_files_api::{Connection, Message};

use crate::common::{ANSWER_TIMEOUT, Bench};

pub struct Feature<E> {
    pub connection: Connection,
    pub events: Receiver<E>,
}

impl<E: Message> Feature<E> {
    pub fn of(bench: &Bench) -> Self {
        let (sink, events) = mpsc::channel();
        let connection = bench.core.connector.connect(sink).unwrap();
        Self { connection, events }
    }

    pub fn next(&self) -> E {
        self.events.recv_timeout(ANSWER_TIMEOUT).unwrap()
    }

    pub fn send(&self, command: impl Message) {
        self.connection.send(command).unwrap();
    }
}
