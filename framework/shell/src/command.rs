use std::fmt;

use gpui::{Action, KeyBinding};

pub const WINDOW_KEYS: &str = "MainWindow";

pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub keys: &'static [&'static str],
    bind: fn(&str, Option<&str>) -> KeyBinding,
    make: fn() -> Box<dyn Action>,
}

impl CommandSpec {
    pub const fn new(
        (id, label, keys): (&'static str, &'static str, &'static [&'static str]),
        bind: fn(&str, Option<&str>) -> KeyBinding,
        make: fn() -> Box<dyn Action>,
    ) -> Self {
        Self {
            id,
            label,
            keys,
            bind,
            make,
        }
    }

    pub fn action(&self) -> Box<dyn Action> {
        (self.make)()
    }

    pub fn binding(&self, keys: &str) -> KeyBinding {
        (self.bind)(keys, Some(WINDOW_KEYS))
    }

    pub fn bindings(&self) -> impl Iterator<Item = KeyBinding> + '_ {
        self.keys.iter().map(|keys| self.binding(keys))
    }
}

impl PartialEq for CommandSpec {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for CommandSpec {}

impl fmt::Debug for CommandSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id)
    }
}

#[macro_export]
macro_rules! command_spec {
    ($(#[$doc:meta])* $name:ident, $action:ident, $id:literal, $label:literal, [$($keys:literal),*]) => {
        $(#[$doc])*
        pub const $name: $crate::CommandSpec = $crate::CommandSpec::new(
            ($id, $label, &[$($keys),*]),
            |keys, context| gpui::KeyBinding::new(keys, $action, context),
            || Box::new($action),
        );
    };
}
