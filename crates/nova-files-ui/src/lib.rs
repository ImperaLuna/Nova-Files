//! The window. It sends commands to the core and draws from its events; it
//! never touches the disk.
//!
//! [`run`] takes over the calling thread with the GPUI app, opens the first
//! window, and then follows the [`Request`]s the program sends it: another
//! window, or quit.

#![forbid(unsafe_code)]

mod requests;
mod theme;
mod window;

use std::io::{self, Write as _};
use std::sync::mpsc::Receiver;

use futures::StreamExt as _;
use gpui::{App, AppContext as _, Application, Bounds, WindowBounds, WindowOptions, size};
use nova_files_api::Connector;

pub use window::{CoreStatus, MainWindow};

/// What the program asks of the running UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Open another window.
    OpenWindow,
    /// Close every window and return from [`run`].
    Quit,
}

/// Runs the UI on this thread until a [`Request::Quit`] arrives or
/// `requests` closes. Opens one window straight away.
pub fn run(connector: Connector, requests: Receiver<Request>) {
    Application::new().run(move |cx: &mut App| {
        open_window(&connector, cx);
        let mut requests = requests::forward(requests);
        cx.spawn(async move |cx| {
            while let Some(request) = requests.next().await {
                let handled = cx.update(|cx| match request {
                    Request::OpenWindow => open_window(&connector, cx),
                    Request::Quit => cx.quit(),
                });
                if handled.is_err() {
                    return;
                }
            }
            cx.update(|cx| cx.quit()).ok();
        })
        .detach();
    });
}

/// Opens a main window centred on the main display.
pub fn open_window(connector: &Connector, cx: &mut App) {
    let bounds = Bounds::centered(None, size(theme::window::WIDTH, theme::window::HEIGHT), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        app_id: Some(nova_files_api::APP_ID.to_owned()),
        ..WindowOptions::default()
    };
    let connector = connector.clone();
    let opened = cx.open_window(options, move |_window, cx| {
        cx.new(|cx| MainWindow::new(&connector, cx))
    });
    if let Err(error) = opened {
        // The display refused a window; the open ones carry on. Until there
        // is a log, stderr is where this goes.
        writeln!(
            io::stderr(),
            "nova-files: could not open a window: {error:#}"
        )
        .ok();
    }
}
