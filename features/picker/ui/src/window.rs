use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use futures::channel::oneshot;
use gpui::{App, Context, Window};
use nova_files_api::{Command, Connector, Event};
use nova_files_picker_api::{PickAnswer, PickId, PickKind, PickReply, PickRequest};
use nova_files_shell::{CloseWhen, FolderPick, Opening, WindowAsk, WindowLook, open_window};

use crate::part::PickerPart;
use crate::size::PickerState;
use crate::{foreign_parent, titlebar};

const FETCH_WAIT: Duration = Duration::from_millis(100);

static NEXT_OWN_PICK: AtomicU64 = AtomicU64::new(u64::MAX);

pub struct PickWindow(pub Box<PickRequest>);

pub struct PickOpening {
    pub id: PickId,
    pub request: RefCell<Option<Box<PickRequest>>>,
}

impl PickOpening {
    pub fn of(request: Box<PickRequest>) -> Opening {
        Opening::of(Self {
            id: request.id,
            request: RefCell::new(Some(request)),
        })
    }
}

impl WindowAsk for PickWindow {
    fn folder(&mut self) -> Option<PathBuf> {
        self.0.folder.take()
    }

    fn look(&self, connector: &Connector, cx: &mut App) -> WindowLook {
        let state = PickerState::fetch(connector);
        let display = cx.primary_display().map(|display| display.bounds().size);
        WindowLook {
            size: Some(state.picker_size(display)),
            titlebar: Some(titlebar(&self.0)),
            foreign_parent: self.0.parent.as_ref().map(foreign_parent),
        }
    }

    fn opening(self: Box<Self>) -> Opening {
        PickOpening::of(self.0)
    }
}

pub fn closes(id: PickId) -> CloseWhen {
    Box::new(move |opening: &Opening| {
        opening
            .get::<PickOpening>()
            .is_some_and(|pick| pick.id == id)
    })
}

impl PickerState {
    pub fn fetch(connector: &Connector) -> Self {
        let (sink, events) = mpsc::channel();
        let Ok(connection) = connector.connect(sink) else {
            return Self::default();
        };
        if connection.send(Command::WindowState).is_err() {
            return Self::default();
        }
        match events.recv_timeout(FETCH_WAIT) {
            Ok(Event::WindowState(bytes)) => Self::parse(&bytes),
            _ => Self::default(),
        }
    }
}

impl PickerPart {
    pub(crate) fn started(
        &mut self,
        opening: &Opening,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let request = opening
            .get::<PickOpening>()
            .and_then(|opening| opening.request.borrow_mut().take());
        let Some(request) = request else {
            return;
        };
        self.start(request, cx);
        if let Some(focus) = self.name_focus(cx) {
            window.focus(&focus, cx);
        }
    }

    #[expect(clippy::unused_self, reason = "a slot is a method of the part")]
    pub(crate) fn pick_folder(
        &mut self,
        pick: FolderPick,
        folder: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> bool {
        let (deliver, answer) = oneshot::channel();
        let request = PickRequest {
            id: PickId(NEXT_OWN_PICK.fetch_sub(1, Ordering::Relaxed)),
            kind: PickKind::Open {
                multiple: false,
                folders: true,
            },
            title: pick.title.to_string(),
            accept_label: Some(pick.accept.to_string()),
            filters: Vec::new(),
            filter: None,
            folder,
            parent: None,
            types: None,
            reply: PickReply::new(move |picked| drop(deliver.send(picked))),
        };
        cx.defer(move |cx| open_window(Box::new(PickWindow(Box::new(request))), cx));
        cx.spawn(async move |_, cx| {
            let Ok(PickAnswer::Picked { paths, .. }) = answer.await else {
                return;
            };
            if let Some(path) = paths.into_iter().next() {
                cx.update(|cx| {
                    let picked = pick.picked.clone();
                    cx.defer(move |cx| picked(path, cx));
                });
            }
        })
        .detach();
        true
    }
}
