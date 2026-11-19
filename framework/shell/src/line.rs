use futures::StreamExt as _;
use futures::channel::mpsc::{self, UnboundedSender};
use gpui::{Context, Task};
use nova_files_messages::{
    ClientGone, Connection, Connector, CoreGone, Envelope, EventSink, Message,
};

#[derive(Clone, Default)]
pub struct Lines(Option<Connector>);

impl Lines {
    pub const fn new(connector: Option<Connector>) -> Self {
        Self(connector)
    }

    pub fn open<P: 'static>(
        &self,
        cx: &mut Context<P>,
        take: fn(&mut P, Envelope, &mut Context<P>),
    ) -> Line {
        let (sink, mut heard) = mpsc::unbounded();
        let connection = self
            .0
            .as_ref()
            .and_then(|connector| connector.connect(Sink(sink)).ok());
        let listening = cx.spawn(async move |part, cx| {
            while let Some(envelope) = heard.next().await {
                if part
                    .update(cx, |part, cx| take(part, envelope, cx))
                    .is_err()
                {
                    return;
                }
            }
        });
        Line {
            connection,
            _listening: listening,
        }
    }
}

pub struct Line {
    connection: Option<Connection>,
    _listening: Task<()>,
}

impl Line {
    pub fn send<M: Message>(&self, message: M) -> Result<(), CoreGone> {
        self.connection.as_ref().ok_or(CoreGone)?.send(message)
    }
}

struct Sink(UnboundedSender<Envelope>);

impl EventSink for Sink {
    fn deliver_envelope(&self, envelope: Envelope) -> Result<(), ClientGone> {
        self.0.unbounded_send(envelope).map_err(|_| ClientGone)
    }
}
