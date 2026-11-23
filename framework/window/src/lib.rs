// GPUI on Linux ends its run when the last window closes.
// https://github.com/ImperaLuna/zed/tree/610fab89331e30736906b44f33bffe0cedcae37e/crates/gpui
#![forbid(unsafe_code)]

mod band;
mod columns;
mod commands;
mod dropping;
mod edge_scroll;
mod island;
mod lift;
mod scroll_bar;

mod marks;
mod menu;
mod pile;
mod places;
mod requests;
mod selecting;
mod selection;
mod theme_icons;
mod view;
mod window;

use std::cell::Cell;
use std::fmt;
use std::io::{self, Write as _};
use std::path::Path;
use std::rc::Rc;

use gpui::{
    App, AppContext as _, Bounds, Global, Task, WindowBounds, WindowKind, WindowOptions, size,
};
use nova_files_api::{Command, Connector, Event, MessageType, WindowRequest};

pub use assets::Icon;
pub use columns::ColumnText;
pub use commands::{ACTIONS, CommandSpec, bind_keys as bind_window_keys};
pub use island::{NOTE_TIME, Note};
pub use menu::{Look, Menu, MenuAction, MenuItem};
pub use nova_files_shell::Location;
use nova_files_shell::{Asked, CloseWhen, Opener, Parts, assets, jump, text, theme};
pub use requests::{Request, RequestSender, Requests, channel};
pub use text::{TextField, TextFieldEvent, bind_keys as bind_text_keys};
pub use theme::Colors;
pub use window::{CoreStatus, MainWindow, WindowEvent, home_folder};
pub use window::{HOVER_OPEN_AFTER, JOB_SHOWS_AFTER};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    WindowsClosed,
    Quit,
}

pub enum Opening {
    Folder(WindowRequest),
    Part(Asked),
}

impl fmt::Debug for Opening {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Folder(request) => f.debug_tuple("Folder").field(request).finish(),
            Self::Part(_) => f.write_str("Part"),
        }
    }
}

pub type RecoverGpu = Rc<dyn Fn(Option<&Path>)>;

pub fn sent_types() -> Vec<MessageType> {
    vec![MessageType::of::<Command>()]
}

pub fn taken_types() -> Vec<MessageType> {
    vec![MessageType::of::<Event>()]
}

pub fn run(
    connector: &Connector,
    requests: &Requests,
    first: Opening,
    recover_gpu: &RecoverGpu,
    parts: Parts,
) -> Ended {
    let ended = Rc::new(Cell::new(Ended::WindowsClosed));
    let connector = connector.clone();
    let requests = requests.clone();
    let quit = Rc::clone(&ended);
    let recover_gpu = Rc::clone(recover_gpu);
    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            cx.set_global(parts);
            text::bind_keys(cx);
            commands::bind_keys(cx);
            open_window(&connector, first, &recover_gpu, cx);
            let follow = cx.spawn(async move |cx| {
                loop {
                    let request = requests.next().await;
                    let keep_going = cx.update(|cx| match request {
                        Some(Request::OpenWindow(request)) => {
                            open_window(&connector, Opening::Folder(request), &recover_gpu, cx);
                            true
                        }
                        Some(Request::Open(ask)) => {
                            open_window(&connector, Opening::Part(ask), &recover_gpu, cx);
                            true
                        }
                        Some(Request::Close(when)) => {
                            close_where(&when, cx);
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

struct RequestLoop {
    _follow: Task<()>,
}

impl Global for RequestLoop {}

type OpenFolderWindow = Rc<dyn Fn(WindowRequest, &mut App)>;

#[derive(Clone)]
pub(crate) struct FolderWindows(pub(crate) OpenFolderWindow);

impl Global for FolderWindows {}

pub(crate) fn keep_opener(connector: &Connector, cx: &mut App) {
    if cx.has_global::<Opener>() {
        return;
    }
    let parts = connector.clone();
    cx.set_global(Opener(Rc::new(move |ask, cx| {
        let recover_gpu: RecoverGpu = Rc::new(|_: Option<&Path>| {});
        open_window(&parts, Opening::Part(ask), &recover_gpu, cx);
    })));
    let folders = connector.clone();
    cx.set_global(FolderWindows(Rc::new(move |request, cx| {
        let recover_gpu: RecoverGpu = Rc::new(|_: Option<&Path>| {});
        open_window(&folders, Opening::Folder(request), &recover_gpu, cx);
    })));
}

pub fn close_where(when: &CloseWhen, cx: &mut App) {
    for handle in cx.windows() {
        let Some(handle) = handle.downcast::<MainWindow>() else {
            continue;
        };
        let closed = handle.update(cx, |view, window, cx| {
            if view.opened_as(when) {
                view.dismiss(cx);
                window.remove_window();
            }
        });
        if let Err(error) = closed {
            writeln!(
                io::stderr(),
                "nova-files: could not close a window: {error:#}"
            )
            .ok();
        }
    }
}

fn open_window(connector: &Connector, opening: Opening, recover_gpu: &RecoverGpu, cx: &mut App) {
    let bounds = Bounds::centered(None, size(theme::window::WIDTH, theme::window::HEIGHT), cx);
    let (folder, select, ask) = match opening {
        Opening::Folder(request) => (request.folder, request.select, None),
        Opening::Part(mut ask) => (ask.folder(), Vec::new(), Some(ask)),
    };
    let recovers = ask.is_none();
    let mut options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        app_id: Some(nova_files_api::APP_ID.to_owned()),
        ..WindowOptions::default()
    };
    if let Some(ask) = &ask {
        let look = ask.look(connector, cx);
        if let Some(size) = look.size {
            options.window_bounds = Some(WindowBounds::Windowed(Bounds::centered(None, size, cx)));
        }
        options.titlebar = look.titlebar;
        options.foreign_parent = look.foreign_parent;
        if look.over_asker {
            options.kind = WindowKind::Dialog;
        }
    }
    let connector = connector.clone();
    let location = match folder {
        Some(folder) => Location::Folder(folder),
        None if ask.is_some() => Location::Folder(home_folder()),
        None => Location::start(),
    };
    let recover_folder = location.folder().map(Path::to_path_buf);
    let opened = cx.open_window(options, move |window, cx| {
        cx.new(|cx| {
            let opening = ask.map_or_else(
                nova_files_shell::Opening::default,
                nova_files_shell::WindowAsk::opening,
            );
            let mut view =
                MainWindow::opened_with(&connector, &location, home_folder(), opening.clone(), cx);
            if let Some(folder) = location.folder() {
                view.select_on_arrival(folder.to_path_buf(), select);
            }
            if opening.is_some() {
                view.open_as(opening, window, cx);
            }
            view
        })
    });
    if let Err(error) = opened {
        if recovers && cx.windows().is_empty() {
            recover_gpu(recover_folder.as_deref());
        }
        writeln!(
            io::stderr(),
            "nova-files: could not open a window: {error:#}"
        )
        .ok();
    }
}
