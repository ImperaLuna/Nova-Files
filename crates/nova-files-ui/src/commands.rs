//! The window's actions and the keys that start them, in one table.
//!
//! Each action is a row of [`ACTIONS`]: a stable id (`go.up`), the label a
//! button or menu shows, its default keys, and the GPUI action it sends.
//! Buttons, menus and keys all send the action; the window does the work
//! in one handler per action (see `window`), so a key and a click can't
//! drift apart. [`bind_keys`] binds every row's keys under
//! [`KEY_CONTEXT`], which the window sets; a focused text field inside it
//! sets its own context, so the field's keys win while it has focus.
//!
//! Ids are what settings will name to change a key later, so they never
//! change once shipped. Key strings use GPUI's keystroke syntax
//! (`ctrl-shift-a`).

use gpui::{App, KeyBinding};

/// The key context the window sets.
pub const KEY_CONTEXT: &str = "MainWindow";

#[expect(
    clippy::derive_partial_eq_without_eq,
    reason = "GPUI's actions! macro derives PartialEq only"
)]
mod actions {
    gpui::actions!(nova_files, [GoUp]);
}

pub use actions::GoUp;

/// One action the window can take.
pub struct ActionSpec {
    /// Names the action in settings; never changes once shipped.
    pub id: &'static str,
    /// What a button or menu shows for it.
    pub label: &'static str,
    /// The keys that start it unless the user changes them.
    pub keys: &'static [&'static str],
    /// Binds `keys` (one keystroke sequence) to this action in `context`.
    bind: fn(&str, Option<&str>) -> KeyBinding,
}

impl ActionSpec {
    /// The binding of `keys` to this action under the window's context.
    pub fn binding(&self, keys: &str) -> KeyBinding {
        (self.bind)(keys, Some(KEY_CONTEXT))
    }
}

/// Every action the window can take.
pub const ACTIONS: &[ActionSpec] = &[UP];

/// Goes to the parent of the folder on screen.
pub const UP: ActionSpec = ActionSpec {
    id: "go.up",
    label: "Up",
    keys: &["alt-up"],
    bind: |keys, context| KeyBinding::new(keys, GoUp, context),
};

/// Binds every action's default keys.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys(
        ACTIONS
            .iter()
            .flat_map(|spec| spec.keys.iter().map(|keys| spec.binding(keys))),
    );
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn ids_and_keys_are_each_used_once() {
        let mut ids = HashSet::new();
        let mut keys = HashSet::new();
        for spec in ACTIONS {
            assert!(ids.insert(spec.id), "{} is used twice", spec.id);
            for key in spec.keys {
                assert!(keys.insert(*key), "{key} starts two actions");
            }
        }
    }

    #[test]
    fn ids_are_dotted_lowercase_words() {
        for spec in ACTIONS {
            let words: Vec<&str> = spec.id.split('.').collect();
            assert!(words.len() >= 2, "{} has no group", spec.id);
            assert!(
                words.iter().all(|word| !word.is_empty()
                    && word.chars().all(|c| c.is_ascii_lowercase() || c == '-')),
                "{} isn't dotted lowercase words",
                spec.id
            );
        }
    }

    #[test]
    fn every_action_has_a_label_and_keys_that_bind_to_it() {
        for spec in ACTIONS {
            assert!(!spec.label.is_empty(), "{} has no label", spec.id);
            for key in spec.keys {
                // Panics on a key string GPUI can't read.
                let binding = spec.binding(key);
                assert!(!binding.keystrokes().is_empty(), "{key}");
            }
        }
    }
}
