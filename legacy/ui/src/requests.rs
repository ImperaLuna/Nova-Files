use std::cell::RefCell;
use std::rc::Rc;

use futures::StreamExt as _;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use nova_files_api::WindowRequest;
use nova_files_picker_api::{PickId, PickRequest};

#[derive(Debug)]
pub enum Request {
    OpenWindow(WindowRequest),
    Pick(Box<PickRequest>),
    ClosePick(PickId),
    Quit,
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
