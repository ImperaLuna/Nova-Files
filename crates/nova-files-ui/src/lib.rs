//! The window. It sends commands to the core and draws from its events; it
//! never touches the disk. A window opens on the home folder unless the
//! program names another.
//!
//! [`run`] takes over the calling thread with a GPUI app, opens a window,
//! and follows the program's [`Request`]s until the last window closes or
//! a [`Request::Quit`] arrives. GPUI on Linux ends its run when the last
//! window closes, so the program calls [`run`] again for the next window.

#![forbid(unsafe_code)]

mod assets;
mod columns;
mod requests;
mod theme;
mod thumbnail;
mod view;
mod window;

use std::cell::Cell;
use std::io::{self, Write as _};
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    App, AppContext as _, Bounds, Global, Task, WindowBounds, WindowHandle, WindowOptions, size,
};
use nova_files_api::Connector;

pub use columns::ColumnText;
pub use requests::{Request, RequestSender, Requests, channel};
pub use window::{CoreStatus, MainWindow, WindowEvent, home_folder};

/// Why [`run`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// The last window closed; the program carries on without a window.
    WindowsClosed,
    /// A [`Request::Quit`] arrived, or every request sender is gone.
    Quit,
}

/// Asked when a window can't be opened.
///
/// That means its GPU setup failed. It returns whether it changed
/// something that makes one more try worth it; the program uses it to give
/// the Vulkan loader back every driver.
pub type RetryGpu = fn() -> bool;

/// Runs the UI on this thread with one window open on `folder` (or the
/// home folder), until the last window closes or the program asks it to
/// quit.
pub fn run(
    connector: &Connector,
    requests: &Requests,
    folder: Option<PathBuf>,
    retry_gpu: RetryGpu,
) -> Ended {
    let ended = Rc::new(Cell::new(Ended::WindowsClosed));
    let connector = connector.clone();
    let requests = requests.clone();
    let quit = Rc::clone(&ended);
    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            open_window(&connector, folder, retry_gpu, cx);
            let follow = cx.spawn(async move |cx| {
                loop {
                    let request = requests.next().await;
                    let keep_going = cx.update(|cx| match request {
                        Some(Request::OpenWindow(folder)) => {
                            open_window(&connector, folder, retry_gpu, cx);
                            true
                        }
                        Some(Request::Quit) | None => {
                            quit.set(Ended::Quit);
                            cx.quit();
                            false
                        }
                    });
                    if !keep_going {
                        return;
                    }
                }
            });
            cx.set_global(RequestLoop { _follow: follow });
        });
    ended.get()
}

/// The task that follows the program's requests. The app owns it, so it is
/// dropped with the app when the run ends and never touches a released app.
struct RequestLoop {
    _follow: Task<()>,
}

impl Global for RequestLoop {}

/// Opens a main window on `folder` (or the home folder), centred on the
/// main display. When that fails and `retry_gpu` changed something, it
/// tries once more.
fn open_window(connector: &Connector, folder: Option<PathBuf>, retry_gpu: RetryGpu, cx: &mut App) {
    let folder = folder.unwrap_or_else(home_folder);
    let opened = match try_open_window(connector, folder.clone(), cx) {
        Err(_) if retry_gpu() => try_open_window(connector, folder, cx),
        opened => opened,
    };
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

fn try_open_window(
    connector: &Connector,
    folder: PathBuf,
    cx: &mut App,
) -> gpui::Result<WindowHandle<MainWindow>> {
    let bounds = Bounds::centered(None, size(theme::window::WIDTH, theme::window::HEIGHT), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        app_id: Some(nova_files_api::APP_ID.to_owned()),
        ..WindowOptions::default()
    };
    let connector = connector.clone();
    cx.open_window(options, move |_window, cx| {
        cx.new(|cx| MainWindow::new(&connector, &folder, cx))
    })
}
