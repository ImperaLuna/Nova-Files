//! `org.freedesktop.Application`, the freedesktop interface a running app
//! offers so a second launch (or a launcher using D-Bus activation) can
//! hand over to it instead of starting a second process.
//!
//! Only `Activate` is served for now: it asks for a new window. `Open` and
//! `ActivateAction` come with the features that need them.

use std::collections::HashMap;

use zbus::zvariant::OwnedValue;

pub const INTERFACE: &str = "org.freedesktop.Application";

pub struct Application<F> {
    on_activate: F,
}

impl<F> Application<F> {
    pub const fn new(on_activate: F) -> Self {
        Self { on_activate }
    }
}

#[zbus::interface(name = "org.freedesktop.Application")]
impl<F: Fn() + Send + Sync + 'static> Application<F> {
    /// Opens a new window. `platform_data` (startup notification ids and
    /// the like) is not used yet.
    fn activate(&self, platform_data: HashMap<String, OwnedValue>) {
        drop(platform_data);
        (self.on_activate)();
    }
}
