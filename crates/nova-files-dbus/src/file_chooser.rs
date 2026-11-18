use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use futures::channel::oneshot;
use nova_files_api::{ParentWindow, PickAnswer, PickId, PickReply, PickRequest};
use zbus::message::Header;
use zbus::object_server::ObjectServer;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

use crate::holds::Holds;
use crate::portal_options::{self, FAILED, Options, Parsed};

pub const BUS_NAME: &str = env!("NOVA_FILES_PORTAL_BUS_NAME");

// The path, interface and methods are xdg-desktop-portal's FileChooser backend API.
// https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.impl.portal.FileChooser.html
pub const OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";

pub const INTERFACE: &str = "org.freedesktop.impl.portal.FileChooser";

pub trait Picker: Send + Sync + 'static {
    fn pick(&self, request: PickRequest);
    fn close(&self, id: PickId);
}

pub struct FileChooser<P> {
    picker: Arc<P>,
    holds: Holds,
    next: AtomicU64,
}

impl<P: Picker> FileChooser<P> {
    pub fn new(picker: P, holds: Holds) -> Self {
        Self {
            picker: Arc::new(picker),
            holds,
            next: AtomicU64::new(0),
        }
    }

    async fn pick(
        &self,
        header: &Header<'_>,
        server: &ObjectServer,
        handle: OwnedObjectPath,
        parsed: Parsed,
    ) -> (u32, Options) {
        let id = PickId(self.next.fetch_add(1, Ordering::Relaxed));
        let closed = Arc::new(Mutex::new(false));
        let request_object = Request {
            id,
            picker: Arc::clone(&self.picker),
            closed: Arc::clone(&closed),
        };
        if !matches!(server.at(&handle, request_object).await, Ok(true)) {
            return (FAILED, Options::new());
        }
        let filters = parsed.filters.clone();
        let answer = self.answer(header, id, &closed, parsed).await;
        server.remove::<Request<P>, _>(&handle).await.ok();
        answer.map_or_else(
            || (FAILED, Options::new()),
            |answer| portal_options::results(&answer, &filters),
        )
    }

    async fn answer(
        &self,
        header: &Header<'_>,
        id: PickId,
        closed: &Mutex<bool>,
        parsed: Parsed,
    ) -> Option<PickAnswer> {
        let caller = header.sender()?;
        let hold = self.holds.take(caller).await.ok()?;
        let (sender, answer) = oneshot::channel();
        let request = PickRequest {
            id,
            kind: parsed.kind,
            title: parsed.title,
            accept_label: parsed.accept_label,
            filters: parsed.filters,
            filter: parsed.filter,
            folder: parsed.folder,
            parent: parsed.parent,
            types: None,
            reply: PickReply::new(move |answer| {
                sender.send(answer).ok();
            }),
        };
        if !show_unless_closed(&*self.picker, closed, request) {
            return Some(PickAnswer::Cancelled);
        }
        let answer = answer.await.unwrap_or(PickAnswer::Cancelled);
        drop(hold);
        Some(answer)
    }
}

#[expect(
    clippy::significant_drop_tightening,
    reason = "the lock is held across the hand-off on purpose"
)]
fn show_unless_closed(picker: &impl Picker, closed: &Mutex<bool>, request: PickRequest) -> bool {
    let closed = lock(closed);
    if *closed {
        return false;
    }
    picker.pick(request);
    true
}

fn lock(closed: &Mutex<bool>) -> MutexGuard<'_, bool> {
    closed.lock().unwrap_or_else(PoisonError::into_inner)
}

#[zbus::interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl<P: Picker> FileChooser<P> {
    #[expect(
        clippy::too_many_arguments,
        reason = "the portal's method signature, plus what zbus passes in"
    )]
    async fn open_file(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(object_server)] server: &ObjectServer,
        handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        title: String,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        drop(app_id);
        let mut parsed = portal_options::open(title, &options);
        parsed.parent = ParentWindow::parse(&parent_window);
        self.pick(&header, server, handle, parsed).await
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the portal's method signature, plus what zbus passes in"
    )]
    async fn save_file(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(object_server)] server: &ObjectServer,
        handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        title: String,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        drop(app_id);
        let mut parsed = portal_options::save(title, &options);
        parsed.parent = ParentWindow::parse(&parent_window);
        self.pick(&header, server, handle, parsed).await
    }
}

struct Request<P> {
    id: PickId,
    picker: Arc<P>,
    closed: Arc<Mutex<bool>>,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Request")]
impl<P: Picker> Request<P> {
    #[expect(
        clippy::significant_drop_tightening,
        reason = "held while the picker is told, so a pick can't slip in between"
    )]
    fn close(&self) {
        let mut closed = lock(&self.closed);
        *closed = true;
        self.picker.close(self.id);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use nova_files_api::{PickId, PickKind, PickReply, PickRequest};
    use zbus::object_server::Interface as _;

    use super::{FileChooser, INTERFACE, Picker, Request, lock, show_unless_closed};

    struct Nothing;

    impl Picker for Nothing {
        fn pick(&self, request: PickRequest) {
            drop(request);
        }
        fn close(&self, _id: PickId) {}
    }

    #[derive(Default)]
    struct Counts {
        picks: Mutex<u32>,
        closes: Mutex<u32>,
    }

    impl Picker for Counts {
        fn pick(&self, request: PickRequest) {
            drop(request);
            *lock_count(&self.picks) += 1;
        }
        fn close(&self, _id: PickId) {
            *lock_count(&self.closes) += 1;
        }
    }

    fn lock_count(count: &Mutex<u32>) -> std::sync::MutexGuard<'_, u32> {
        count
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn request() -> PickRequest {
        PickRequest {
            id: PickId(1),
            kind: PickKind::Open {
                multiple: false,
                folders: false,
            },
            title: String::new(),
            accept_label: None,
            filters: Vec::new(),
            filter: None,
            folder: None,
            parent: None,
            types: None,
            reply: PickReply::new(drop),
        }
    }

    #[test]
    fn interface_name_matches_the_constant() {
        assert_eq!(FileChooser::<Nothing>::name().as_str(), INTERFACE);
    }

    #[test]
    fn a_pick_closed_before_it_shows_never_shows() {
        let picker = Arc::new(Counts::default());
        let closed = Arc::new(Mutex::new(false));
        let request_object = Request {
            id: PickId(1),
            picker: Arc::clone(&picker),
            closed: Arc::clone(&closed),
        };

        request_object.close();
        assert!(*lock(&closed));
        assert_eq!(*lock_count(&picker.closes), 1);

        assert!(!show_unless_closed(&*picker, &closed, request()));
        assert_eq!(*lock_count(&picker.picks), 0);
    }

    #[test]
    fn a_pick_not_closed_shows() {
        let picker = Counts::default();
        assert!(show_unless_closed(&picker, &Mutex::new(false), request()));
        assert_eq!(*lock_count(&picker.picks), 1);
    }
}
