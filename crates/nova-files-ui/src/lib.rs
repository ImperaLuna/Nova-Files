//! The window. It sends commands to the core and draws from its events; it
//! never touches the disk. A window opens on the home folder unless the
//! program names another. A picker window is the same window in pick mode,
//! opened for another app's request (see `pick`).
//!
//! [`run`] takes over the calling thread with a GPUI app, opens a window,
//! and follows the program's [`Request`]s until the last window closes or
//! a [`Request::Quit`] arrives. GPUI on Linux ends its run when the last
//! window closes, so the program calls [`run`] again for the next window.

#![forbid(unsafe_code)]

mod assets;
mod columns;
mod pick;
mod requests;
mod shown_thumbnails;
mod text;
mod theme;
mod thumbnail;
mod view;
mod window;
mod window_state;

use std::cell::Cell;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{App, AppContext as _, Bounds, Global, Task, WindowBounds, WindowOptions, size};
use nova_files_api::{Connector, PickRequest};

pub use columns::ColumnText;
pub use pick::close as close_picker;
pub use requests::{Request, RequestSender, Requests, channel};
pub use text::{TextField, TextFieldEvent, bind_keys as bind_text_keys};
pub use theme::Colors;
pub use window::{CoreStatus, MainWindow, WindowEvent, home_folder};
pub use window_state::{WindowState, fetch as fetch_window_state};

/// Why [`run`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// The last window closed; the program carries on without a window.
    WindowsClosed,
    /// A [`Request::Quit`] arrived, or every request sender is gone.
    Quit,
}

/// What the first window of a run opens on.
#[derive(Debug)]
pub enum Opening {
    /// A main window on this folder, or on the home folder.
    Folder(Option<PathBuf>),
    /// A picker window for another app's request.
    Pick(Box<PickRequest>),
}

/// Called with the folder when the first window can't be opened, which
/// means its GPU setup failed. The program uses it to start again with
/// every Vulkan driver; it returns only when it didn't.
pub type RecoverGpu = Rc<dyn Fn(&Path)>;

/// Runs the UI on this thread with one window open as `first` asks, until
/// the last window closes or the program asks it to quit.
pub fn run(
    connector: &Connector,
    requests: &Requests,
    first: Opening,
    recover_gpu: &RecoverGpu,
) -> Ended {
    let ended = Rc::new(Cell::new(Ended::WindowsClosed));
    let connector = connector.clone();
    let requests = requests.clone();
    let quit = Rc::clone(&ended);
    let recover_gpu = Rc::clone(recover_gpu);
    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            text::bind_keys(cx);
            open_window(&connector, first, &recover_gpu, cx);
            let follow = cx.spawn(async move |cx| {
                loop {
                    let request = requests.next().await;
                    let keep_going = cx.update(|cx| match request {
                        Some(Request::OpenWindow(folder)) => {
                            open_window(&connector, Opening::Folder(folder), &recover_gpu, cx);
                            true
                        }
                        Some(Request::Pick(request)) => {
                            open_window(&connector, Opening::Pick(request), &recover_gpu, cx);
                            true
                        }
                        Some(Request::ClosePick(id)) => {
                            pick::close(id, cx);
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

/// Opens a main window on its folder (or the home folder), or a picker
/// window titled as the asking app wants and dialog-sized, over the app's
/// window when it names one, else centred on the main display.
/// When a main window fails with no window open, `recover_gpu` gets the
/// folder. A picker that fails to open answers cancelled: restarting
/// would lose the asking app's call.
fn open_window(connector: &Connector, opening: Opening, recover_gpu: &RecoverGpu, cx: &mut App) {
    let bounds = Bounds::centered(None, size(theme::window::WIDTH, theme::window::HEIGHT), cx);
    let (folder, pick) = match opening {
        Opening::Folder(folder) => (folder, None),
        Opening::Pick(mut request) => (request.folder.take(), Some(request)),
    };
    let recovers = pick.is_none();
    let mut options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        app_id: Some(nova_files_api::APP_ID.to_owned()),
        ..WindowOptions::default()
    };
    if let Some(request) = &pick {
        let state = window_state::fetch(connector);
        let display = cx.primary_display().map(|display| display.bounds().size);
        let picker_bounds = Bounds::centered(None, state.picker_size(display), cx);
        options.window_bounds = Some(WindowBounds::Windowed(picker_bounds));
        options.titlebar = Some(pick::titlebar(request));
        options.foreign_parent = request.parent.as_ref().map(pick::foreign_parent);
    }
    let connector = connector.clone();
    let folder = folder.unwrap_or_else(home_folder);
    let recover_folder = folder.clone();
    let opened = cx.open_window(options, move |window, cx| {
        cx.new(|cx| {
            let mut view = MainWindow::new(&connector, &folder, cx);
            if let Some(request) = pick {
                view.set_pick(request, window, cx);
            }
            view
        })
    });
    if let Err(error) = opened {
        if recovers && cx.windows().is_empty() {
            recover_gpu(&recover_folder);
        }
        // The display refused a window; any open ones carry on. Until there
        // is a log, stderr is where this goes.
        writeln!(
            io::stderr(),
            "nova-files: could not open a window: {error:#}"
        )
        .ok();
    }
}
