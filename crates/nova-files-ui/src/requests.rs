//! Carries the program's requests from a std channel into the GPUI app.
//!
//! The program sends from plain threads (D-Bus, the core's watcher); GPUI
//! tasks wait on async channels. One thread blocks on the std receiver and
//! passes each request on, so nothing polls.

use std::sync::mpsc::Receiver;
use std::thread;

use futures::channel::mpsc::{self, UnboundedReceiver};

use crate::Request;

const THREAD_NAME: &str = "nova-files-ui-requests";

/// Returns an async stream of what arrives on `requests`. It ends when
/// `requests` closes.
pub fn forward(requests: Receiver<Request>) -> UnboundedReceiver<Request> {
    let (sender, receiver) = mpsc::unbounded();
    let spawned = thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            while let Ok(request) = requests.recv() {
                if sender.unbounded_send(request).is_err() {
                    return;
                }
            }
        });
    // Without the thread the stream ends at once and the UI quits, which is
    // the right outcome when the OS can't start a thread.
    drop(spawned);
    receiver
}
