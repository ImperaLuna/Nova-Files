use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use futures::StreamExt as _;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use nova_files_api::WindowRequest;
use nova_files_shell::{Asked, CloseWhen};

pub enum Request {
    OpenWindow(WindowRequest),
    Open(Asked),
    Close(CloseWhen),
    Quit,
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpenWindow(request) => f.debug_tuple("OpenWindow").field(request).finish(),
            Self::Open(_) => f.write_str("Open"),
            Self::Close(_) => f.write_str("Close"),
            Self::Quit => f.write_str("Quit"),
        }
    }
}

pub fn channel() -> (RequestSender, Requests) {
    let (sender, receiver) = mpsc::unbounded();
    (
        RequestSender(sender),
        Requests(Rc::new(RefCell::new(receiver))),
    )
}

#[derive(Clone)]
pub struct RequestSender(UnboundedSender<Request>);

impl RequestSender {
    pub fn send(&self, request: Request) {
        self.0.unbounded_send(request).ok();
    }
}

#[derive(Clone)]
pub struct Requests(Rc<RefCell<UnboundedReceiver<Request>>>);

impl Requests {
    pub fn wait(&self) -> Option<Request> {
        futures::executor::block_on(self.next())
    }

    #[expect(
        clippy::future_not_send,
        reason = "the receiver stays on the main thread, with GPUI's tasks"
    )]
    pub async fn next(&self) -> Option<Request> {
        futures::future::poll_fn(|cx| self.0.borrow_mut().poll_next_unpin(cx)).await
    }
}
