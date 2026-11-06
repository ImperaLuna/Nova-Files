//! The window. It sends commands to the core and draws from its events; it
//! never touches the disk.
//!
//! [`run`] takes over the calling thread with a GPUI app, opens a window,
//! and follows the program's [`Request`]s until the last window closes or
//! a [`Request::Quit`] arrives. GPUI on Linux ends its run when the last
//! window closes, so the program calls [`run`] again for the next window.

#![forbid(unsafe_code)]

mod requests;
mod theme;
mod window;

use std::cell::Cell;
use std::io::{self, Write as _};
use std::rc::Rc;

use gpui::{App, AppContext as _, Application, Bounds, WindowBounds, WindowOptions, size};
use nova_files_api::Connector;

pub use requests::{Request, RequestSender, Requests, channel};
pub use window::{CoreStatus, MainWindow};

/// Why [`run`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// The last window closed; the program carries on without a window.
    WindowsClosed,
    /// A [`Request::Quit`] arrived, or every request sender is gone.
    Quit,
}

/// Runs the UI on this thread with one window open, until the last window
/// closes or the program asks it to quit.
pub fn run(connector: &Connector, requests: &Requests) -> Ended {
    let ended = Rc::new(Cell::new(Ended::WindowsClosed));
    let connector = connector.clone();
    let requests = requests.clone();
    let quit = Rc::clone(&ended);
    Application::new().run(move |cx: &mut App| {
        open_window(&connector, cx);
        cx.spawn(async move |cx| {
            loop {
                let request = requests.next().await;
                let handled = cx.update(|cx| match request {
                    Some(Request::OpenWindow) => open_window(&connector, cx),
                    Some(Request::Quit) | None => {
                        quit.set(Ended::Quit);
                        cx.quit();
                    }
                });
                if handled.is_err() {
                    return;
                }
            }
        })
        .detach();
    });
    ended.get()
}

/// Opens a main window centred on the main display.
fn open_window(connector: &Connector, cx: &mut App) {
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
        // The display refused a window; any open ones carry on. Until there
        // is a log, stderr is where this goes.
        writeln!(
            io::stderr(),
            "nova-files: could not open a window: {error:#}"
        )
        .ok();
    }
}
