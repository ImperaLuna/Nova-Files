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
    gpui::actions!(
        nova_files,
        [
            GoUp,
            GoBack,
            GoForward,
            OpenSelected,
            CursorUp,
            CursorDown,
            CursorPageUp,
            CursorPageDown,
            CursorFirst,
            CursorLast,
            SelectUp,
            SelectDown,
            SelectPageUp,
            SelectPageDown,
            SelectFirst,
            SelectLast,
            SelectAll,
            ToggleSelected,
            ClearSelection,
        ]
    );
}

pub use actions::*;

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

/// Defines one [`ActionSpec`] constant for `$action`.
macro_rules! spec {
    ($(#[$doc:meta])* $name:ident, $action:ident, $id:literal, $label:literal, [$($keys:literal),*]) => {
        $(#[$doc])*
        pub const $name: ActionSpec = ActionSpec {
            id: $id,
            label: $label,
            keys: &[$($keys),*],
            bind: |keys, context| KeyBinding::new(keys, $action, context),
        };
    };
}

/// Every action the window can take.
pub const ACTIONS: &[ActionSpec] = &[
    UP,
    BACK,
    FORWARD,
    OPEN,
    CURSOR_UP,
    CURSOR_DOWN,
    CURSOR_PAGE_UP,
    CURSOR_PAGE_DOWN,
    CURSOR_FIRST,
    CURSOR_LAST,
    SELECT_UP,
    SELECT_DOWN,
    SELECT_PAGE_UP,
    SELECT_PAGE_DOWN,
    SELECT_FIRST,
    SELECT_LAST,
    SELECT_ALL,
    TOGGLE_SELECTED,
    CLEAR_SELECTION,
];

spec!(
    /// Goes to the parent of the folder on screen.
    UP, GoUp, "go.up", "Up", ["alt-up"]
);
spec!(
    /// Goes back to the place before this one.
    BACK, GoBack, "go.back", "Back", ["alt-left", "backspace"]
);
spec!(
    /// Goes forward again after Back.
    FORWARD, GoForward, "go.forward", "Forward", ["alt-right"]
);
spec!(
    /// Goes into the folder selected, or opens the files selected.
    OPEN, OpenSelected, "entry.open", "Open", ["enter"]
);
spec!(CURSOR_UP, CursorUp, "cursor.up", "Previous", ["up"]);
spec!(CURSOR_DOWN, CursorDown, "cursor.down", "Next", ["down"]);
spec!(
    CURSOR_PAGE_UP,
    CursorPageUp,
    "cursor.page-up",
    "Page up",
    ["pageup"]
);
spec!(
    CURSOR_PAGE_DOWN,
    CursorPageDown,
    "cursor.page-down",
    "Page down",
    ["pagedown"]
);
spec!(CURSOR_FIRST, CursorFirst, "cursor.first", "First", ["home"]);
spec!(CURSOR_LAST, CursorLast, "cursor.last", "Last", ["end"]);
spec!(
    SELECT_UP,
    SelectUp,
    "select.up",
    "Select previous",
    ["shift-up"]
);
spec!(
    SELECT_DOWN,
    SelectDown,
    "select.down",
    "Select next",
    ["shift-down"]
);
spec!(
    SELECT_PAGE_UP,
    SelectPageUp,
    "select.page-up",
    "Select page up",
    ["shift-pageup"]
);
spec!(
    SELECT_PAGE_DOWN,
    SelectPageDown,
    "select.page-down",
    "Select page down",
    ["shift-pagedown"]
);
spec!(
    SELECT_FIRST,
    SelectFirst,
    "select.first",
    "Select to first",
    ["shift-home"]
);
spec!(
    SELECT_LAST,
    SelectLast,
    "select.last",
    "Select to last",
    ["shift-end"]
);
spec!(
    SELECT_ALL,
    SelectAll,
    "select.all",
    "Select all",
    ["ctrl-a"]
);
spec!(
    /// Adds the entry at the cursor to the selection or takes it out.
    TOGGLE_SELECTED, ToggleSelected, "select.toggle", "Select or unselect", ["ctrl-space"]
);
spec!(
    CLEAR_SELECTION,
    ClearSelection,
    "select.none",
    "Select none",
    ["escape"]
);

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
