use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::hand_edited::lock;

pub struct ChangeWriter<T> {
    thread: &'static str,
    what: &'static str,
    apply: Arc<dyn Fn(T) + Send + Sync>,
    queue: Mutex<Option<Sender<T>>>,
}

impl<T: Send + 'static> ChangeWriter<T> {
    pub fn new(
        thread: &'static str,
        what: &'static str,
        apply: impl Fn(T) + Send + Sync + 'static,
    ) -> Self {
        Self {
            thread,
            what,
            apply: Arc::new(apply),
            queue: Mutex::new(None),
        }
    }

    pub fn send(&self, change: T) {
        let mut queue = lock(&self.queue);
        let change = match queue.as_ref() {
            Some(running) => match running.send(change) {
                Ok(()) => return,
                Err(mpsc::SendError(change)) => change,
            },
            None => change,
        };
        *queue = self.start();
        if queue
            .as_ref()
            .is_some_and(|started| started.send(change).is_err())
        {
            log::warn!("the {} writer stopped; changes aren't saved", self.what);
            *queue = None;
        }
    }

    fn start(&self) -> Option<Sender<T>> {
        let (queue, changes) = mpsc::channel::<T>();
        let apply = Arc::clone(&self.apply);
        let started = thread::Builder::new()
            .name(self.thread.to_owned())
            .spawn(move || {
                for change in changes {
                    apply(change);
                }
            });
        match started {
            Ok(_) => Some(queue),
            Err(error) => {
                log::warn!(
                    "can't start the {} writer; changes aren't saved: {error}",
                    self.what
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn changes_are_made_in_order_and_survive_the_writer_being_dropped() {
        let (made, seen) = mpsc::channel();
        let (go, gate) = mpsc::channel::<()>();
        let made = Mutex::new(made);
        let gate = Mutex::new(gate);
        let writer = ChangeWriter::new("test-writer", "test", move |change: u32| {
            lock(&gate).recv_timeout(Duration::from_secs(5)).ok();
            lock(&made).send(change).ok();
        });
        for change in 0..5 {
            writer.send(change);
        }
        drop(writer);
        for _ in 0..5 {
            go.send(()).unwrap();
        }
        let all: Vec<u32> = (0..5)
            .map(|_| seen.recv_timeout(Duration::from_secs(5)).unwrap())
            .collect();
        assert_eq!(all, [0, 1, 2, 3, 4]);
    }
}
