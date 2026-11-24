use std::fmt;

use gpui::{Action, KeyBinding, Keystroke};

use crate::rules::Allow;
use crate::text::KEY_CONTEXT as TEXT_KEYS;

pub const WINDOW_KEYS: &str = "MainWindow";

pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub keys: &'static [&'static str],
    pub needs: &'static [Allow],
    bind: fn(&'static str, Option<&str>) -> KeyBinding,
    make: fn() -> Box<dyn Action>,
}

impl CommandSpec {
    pub const fn new(
        (id, label, keys): (&'static str, &'static str, &'static [&'static str]),
        bind: fn(&'static str, Option<&str>) -> KeyBinding,
        make: fn() -> Box<dyn Action>,
    ) -> Self {
        Self {
            id,
            label,
            keys,
            needs: &[],
            bind,
            make,
        }
    }

    #[must_use]
    pub const fn needing(self, needs: &'static [Allow]) -> Self {
        Self { needs, ..self }
    }

    pub fn action(&self) -> Box<dyn Action> {
        (self.make)()
    }

    pub fn binding(&self, keys: &'static str) -> KeyBinding {
        if types_text(keys) {
            (self.bind)(keys, Some(&format!("{WINDOW_KEYS} && !{TEXT_KEYS}")))
        } else {
            (self.bind)(keys, Some(WINDOW_KEYS))
        }
    }

    pub fn bindings(&self) -> impl Iterator<Item = KeyBinding> + '_ {
        self.keys.iter().map(|keys| self.binding(keys))
    }
}

fn types_text(keys: &'static str) -> bool {
    const SPACE: &str = "space";
    Keystroke::parse(keys).is_ok_and(|stroke| {
        let held = stroke.modifiers;
        let typed = stroke.key.chars().count() == 1 || stroke.key == SPACE;
        typed && !held.control && !held.alt && !held.platform && !held.function
    })
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
    ($(#[$doc:meta])* $name:ident, $action:ident, $id:literal, $label:literal, [$($keys:literal),*], needs [$($need:ident),+]) => {
        $(#[$doc])*
        pub const $name: $crate::CommandSpec = $crate::CommandSpec::new(
            ($id, $label, &[$($keys),*]),
            |keys, context| gpui::KeyBinding::new(keys, $action, context),
            || Box::new($action),
        )
        .needing(&[$($crate::Allow::$need),+]);
    };
}
