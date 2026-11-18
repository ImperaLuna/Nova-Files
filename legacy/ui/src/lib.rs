// GPUI on Linux ends its run when the last window closes.
// https://github.com/ImperaLuna/zed/tree/nova/v0.233.10/crates/gpui
#![forbid(unsafe_code)]

mod assets;
mod band;
mod columns;
mod commands;
mod crumbs;
mod drives;
mod dropping;
mod grid;
mod history;
mod island;
mod jump;
mod kind;
mod location;
mod marks;
mod menu;
mod path_bar;
mod pick;
mod places;
mod requests;
mod selecting;
mod selection;
mod shown_thumbnails;
mod text;
mod theme;
mod theme_icons;
mod thumbnail;
mod tree;
mod view;
mod window;
mod window_state;

use std::cell::Cell;
use std::io::{self, Write as _};
use std::path::Path;
use std::rc::Rc;

use gpui::{App, AppContext as _, Bounds, Global, Task, WindowBounds, WindowOptions, size};
use nova_files_api::{Command, Connector, Event, MessageType, WindowRequest};
use nova_files_picker_api::PickRequest;

pub use assets::Icon;
pub use columns::ColumnText;
pub use commands::{ACTIONS, ActionSpec, bind_keys as bind_window_keys};
pub use island::{NOTE_TIME, Note, NoteUndo};
pub use location::Location;
pub use menu::{Look, Menu, MenuAction, MenuItem};
pub use pick::close as close_picker;
pub use requests::{Request, RequestSender, Requests, channel};
pub use text::{TextField, TextFieldEvent, bind_keys as bind_text_keys};
pub use theme::Colors;
pub use view::TopFacts;
pub use window::{
    AccessLevel, CoreStatus, MainWindow, Previewed, PropertiesPage, Scope, Who, WindowEvent,
    home_folder, shared_level,
};
pub use window::{HOVER_OPEN_AFTER, JOB_SHOWS_AFTER};
pub use window_state::{WindowState, fetch as fetch_window_state};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    WindowsClosed,
    Quit,
}

#[derive(Debug)]
pub enum Opening {
    Folder(WindowRequest),
    Pick(Box<PickRequest>),
}

pub type RecoverGpu = Rc<dyn Fn(Option<&Path>)>;

pub fn sent_types() -> Vec<MessageType> {
    vec![
        MessageType::of::<Command>(),
        MessageType::of::<nova_files_drives_api::Command>(),
        MessageType::of::<nova_files_trash_api::Command>(),
    ]
}

pub fn taken_types() -> Vec<MessageType> {
    vec![
        MessageType::of::<Event>(),
        MessageType::of::<nova_files_drives_api::Event>(),
        MessageType::of::<nova_files_trash_api::Event>(),
    ]
}

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

struct RequestLoop {
    _follow: Task<()>,
}

impl Global for RequestLoop {}

pub(crate) fn open_picker(connector: &Connector, request: Box<PickRequest>, cx: &mut App) {
    let recover_gpu: RecoverGpu = Rc::new(|_: Option<&Path>| {});
    open_window(connector, Opening::Pick(request), &recover_gpu, cx);
}

fn open_window(connector: &Connector, opening: Opening, recover_gpu: &RecoverGpu, cx: &mut App) {
    let bounds = Bounds::centered(None, size(theme::window::WIDTH, theme::window::HEIGHT), cx);
    let (folder, select, pick) = match opening {
        Opening::Folder(request) => (request.folder, request.select, None),
        Opening::Pick(mut request) => (request.folder.take(), Vec::new(), Some(request)),
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
    let location = match folder {
        Some(folder) => Location::Folder(folder),
        None if pick.is_some() => Location::Folder(home_folder()),
        None => Location::Start,
    };
    let recover_folder = location.folder().map(Path::to_path_buf);
    let opened = cx.open_window(options, move |window, cx| {
        cx.new(|cx| {
            let mut view = MainWindow::at(&connector, &location, home_folder(), cx);
            if let Some(folder) = location.folder() {
                view.select_on_arrival(folder.to_path_buf(), select);
            }
            if let Some(request) = pick {
                view.set_pick(request, window, cx);
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
