//! The main window. For now it only connects to the core, says hello with a
//! ping, and draws its surface.
//!
//! The core's events arrive through an async channel the window's own task
//! reads; closing the window drops the connection, which tells the core
//! one client fewer is using it.

use futures::StreamExt as _;
use futures::channel::mpsc::{self, UnboundedSender};
use gpui::{Context, EventEmitter, IntoElement, Render, Styled as _, Task, Window, div, rgb};
use nova_files_api::{ClientGone, Command, Connection, Connector, Event, EventSink};

use crate::theme;

/// What the window knows about the core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreStatus {
    /// Connected, waiting for the first answer.
    Connecting,
    /// The core has answered.
    Connected,
    /// The core is not running.
    Gone,
}

pub struct MainWindow {
    status: CoreStatus,
    /// Held so the core counts this window until it closes.
    _connection: Option<Connection>,
    _events: Task<()>,
}

/// Delivers the core's events into the window's async channel.
struct WindowSink(UnboundedSender<Event>);

impl EventSink for WindowSink {
    fn deliver(&self, event: Event) -> Result<(), ClientGone> {
        self.0.unbounded_send(event).map_err(|_| ClientGone)
    }
}

impl MainWindow {
    pub fn new(connector: &Connector, cx: &mut Context<Self>) -> Self {
        let (sink, mut events) = mpsc::unbounded();
        let connection = connector
            .connect(WindowSink(sink))
            .and_then(|connection| connection.send(Command::Ping).map(|()| connection))
            .ok();
        let status = if connection.is_some() {
            CoreStatus::Connecting
        } else {
            CoreStatus::Gone
        };
        let events = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this
                    .update(cx, |this, cx| this.on_event(&event, cx))
                    .is_err()
                {
                    return;
                }
            }
        });
        Self {
            status,
            _connection: connection,
            _events: events,
        }
    }

    pub const fn status(&self) -> CoreStatus {
        self.status
    }

    fn on_event(&mut self, event: &Event, cx: &mut Context<Self>) {
        match event {
            Event::Pong => self.set_status(CoreStatus::Connected, cx),
        }
    }

    fn set_status(&mut self, status: CoreStatus, cx: &mut Context<Self>) {
        if self.status != status {
            self.status = status;
            cx.emit(status);
            cx.notify();
        }
    }
}

impl EventEmitter<CoreStatus> for MainWindow {}

impl Render for MainWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(theme::color::SURFACE))
    }
}
