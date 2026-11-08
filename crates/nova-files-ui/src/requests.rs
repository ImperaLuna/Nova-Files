//! The channel the program uses to ask the UI for windows, for pickers
//! other apps asked for, or to quit.
//!
//! The sending half works from any thread (D-Bus, the core's watcher). The
//! receiving half stays on the main thread and outlives each GPUI run: GPUI
//! ends its run when the last window closes, and the program then waits on
//! the same receiver, without a GPU, for the next request.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use futures::StreamExt as _;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use nova_files_api::{PickId, PickRequest};

/// What the program asks of the UI.
#[derive(Debug)]
pub enum Request {
    /// Open another window, on this folder or on the home folder.
    OpenWindow(Option<PathBuf>),
    /// Open a picker window for another app's request.
    Pick(Box<PickRequest>),
    /// Close the picker window for this request, if it is still open.
    ClosePick(PickId),
    /// Close every window and stop.
    Quit,
}

/// Creates the request channel.
pub fn channel() -> (RequestSender, Requests) {
    let (sender, receiver) = mpsc::unbounded();
    (
        RequestSender(sender),
        Requests(Rc::new(RefCell::new(receiver))),
    )
}

/// Sends requests to the UI. Cheap to clone and safe to share between
/// threads.
#[derive(Clone)]
pub struct RequestSender(UnboundedSender<Request>);

impl RequestSender {
    /// Queues `request`. Dropped silently once the program has stopped
    /// listening, since nothing is left to act on it.
    pub fn send(&self, request: Request) {
        self.0.unbounded_send(request).ok();
    }
}

/// Receives requests, on the main thread only.
#[derive(Clone)]
pub struct Requests(Rc<RefCell<UnboundedReceiver<Request>>>);

impl Requests {
    /// Blocks until the next request; `None` once every sender is gone.
    /// For use while no GPUI app runs on this thread.
    pub fn wait(&self) -> Option<Request> {
        futures::executor::block_on(self.next())
    }

    /// The next request; `None` once every sender is gone. The receiver is
    /// borrowed only while polled, so a task that never resumes (its app
    /// ended) doesn't keep it.
    #[expect(
        clippy::future_not_send,
        reason = "the receiver stays on the main thread, with GPUI's tasks"
    )]
    pub async fn next(&self) -> Option<Request> {
        futures::future::poll_fn(|cx| self.0.borrow_mut().poll_next_unpin(cx)).await
    }
}
