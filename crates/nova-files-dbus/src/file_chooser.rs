//! The file picker: the xdg-desktop-portal `FileChooser` backend, so other
//! apps' Open and Save dialogs open Nova Files.
//!
//! xdg-desktop-portal finds a backend by its own bus name, so the running
//! instance asks for [`BUS_NAME`] beside the application id and serves this
//! interface at [`OBJECT_PATH`], the path every portal backend uses. Its
//! interface name and methods are the portal's, not ours, so it carries no
//! version of its own.
//!
//! `OpenFile` and `SaveFile` read the app's options (`portal_options`),
//! hand a [`PickRequest`] to the [`Picker`] (the UI) and wait for its
//! answer. While they wait, the portal's request object at the call's
//! `handle` takes `Close`, which tells the picker to close that window, and
//! the call holds the core (see `holds`) so service mode stays up until
//! the user answers. `SaveFiles` (several files into one folder) comes
//! later; until then the portal answers it with its own error.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::channel::oneshot;
use nova_files_api::{PickAnswer, PickId, PickReply, PickRequest};
use zbus::message::Header;
use zbus::object_server::ObjectServer;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

use crate::holds::Holds;
use crate::portal_options::{self, FAILED, Options, Parsed};

/// The backend's bus name, set once in `.cargo/config.toml`. A `.portal`
/// file names it, and `portals.conf` picks it by the `.portal` file's name.
pub const BUS_NAME: &str = env!("NOVA_FILES_PORTAL_BUS_NAME");

/// Where xdg-desktop-portal calls every backend.
pub const OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";

/// The interface name. The `#[zbus::interface]` attribute needs the literal
/// too; a test checks the two match.
pub const INTERFACE: &str = "org.freedesktop.impl.portal.FileChooser";

/// What shows the picker: the UI. Both calls come on a D-Bus thread and
/// must hand the work on rather than do it.
pub trait Picker: Send + Sync + 'static {
    /// Shows a picker for `request`, which it answers through its reply.
    fn pick(&self, request: PickRequest);
    /// Closes the picker for `id`, if it is still open; its reply then
    /// answers cancelled.
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

    /// Runs one pick from start to answer.
    async fn pick(
        &self,
        header: &Header<'_>,
        server: &ObjectServer,
        handle: OwnedObjectPath,
        parsed: Parsed,
    ) -> (u32, Options) {
        let Some(caller) = header.sender() else {
            return (FAILED, Options::new());
        };
        let Ok(hold) = self.holds.take(caller).await else {
            return (FAILED, Options::new());
        };
        let id = PickId(self.next.fetch_add(1, Ordering::Relaxed));
        let request_object = Request {
            id,
            picker: Arc::clone(&self.picker),
        };
        // Served first, so a `Close` that comes at once finds it.
        if !matches!(server.at(&handle, request_object).await, Ok(true)) {
            return (FAILED, Options::new());
        }
        let (sender, answer) = oneshot::channel();
        let filters = parsed.filters.clone();
        self.picker.pick(PickRequest {
            id,
            kind: parsed.kind,
            title: parsed.title,
            accept_label: parsed.accept_label,
            filters: parsed.filters,
            filter: parsed.filter,
            folder: parsed.folder,
            reply: PickReply::new(move |answer| {
                sender.send(answer).ok();
            }),
        });
        let answer = answer.await.unwrap_or(PickAnswer::Cancelled);
        server.remove::<Request<P>, _>(&handle).await.ok();
        drop(hold);
        portal_options::results(&answer, &filters)
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl<P: Picker> FileChooser<P> {
    /// Asks the user for files (or folders) to open.
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
        drop((app_id, parent_window));
        let parsed = portal_options::open(title, &options);
        self.pick(&header, server, handle, parsed).await
    }

    /// Asks the user where to save one file.
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
        drop((app_id, parent_window));
        let parsed = portal_options::save(title, &options);
        self.pick(&header, server, handle, parsed).await
    }
}

/// The portal's request object for one pick: the portal calls `Close` on
/// it when the app gives up on the dialog.
struct Request<P> {
    id: PickId,
    picker: Arc<P>,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Request")]
impl<P: Picker> Request<P> {
    /// Closes the picker. The pick's answer then arrives as cancelled, and
    /// the pick removes this object.
    fn close(&self) {
        self.picker.close(self.id);
    }
}

#[cfg(test)]
mod tests {
    use nova_files_api::{PickId, PickRequest};
    use zbus::object_server::Interface as _;

    use super::{FileChooser, INTERFACE, Picker};

    struct Nothing;

    impl Picker for Nothing {
        fn pick(&self, request: PickRequest) {
            drop(request);
        }
        fn close(&self, _id: PickId) {}
    }

    #[test]
    fn interface_name_matches_the_constant() {
        assert_eq!(FileChooser::<Nothing>::name().as_str(), INTERFACE);
    }
}
