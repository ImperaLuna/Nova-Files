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

use gpui::{Action, App, KeyBinding};

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
            CopySelected,
            CutSelected,
            Paste,
            TrashSelected,
            RenameSelected,
            NewFolder,
            Undo,
            TogglePreview,
            PreviewPrevious,
            PreviewNext,
            FocusSearch,
            RestoreSelected,
            EmptyTrash,
            NewTab,
            CloseTab,
            NextTab,
            PreviousTab,
            ToggleHidden,
            OpenMenu,
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
    /// The GPUI action, to send from a menu.
    make: fn() -> Box<dyn Action>,
}

impl ActionSpec {
    /// The GPUI action this row sends.
    pub fn action(&self) -> Box<dyn Action> {
        (self.make)()
    }

    /// The binding of `keys` to this action under the window's context.
    pub fn binding(&self, keys: &str) -> KeyBinding {
        (self.bind)(keys, Some(KEY_CONTEXT))
    }
}

/// Two rows are the same action when they share an id.
impl PartialEq for ActionSpec {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for ActionSpec {}

impl std::fmt::Debug for ActionSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id)
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
            make: || Box::new($action),
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
    COPY,
    CUT,
    PASTE,
    TRASH,
    RENAME,
    NEW_FOLDER,
    UNDO,
    PREVIEW,
    PREVIEW_PREVIOUS,
    PREVIEW_NEXT,
    FIND,
    RESTORE,
    EMPTY_TRASH,
    NEW_TAB,
    CLOSE_TAB,
    NEXT_TAB,
    PREVIOUS_TAB,
    HIDDEN,
    MENU,
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

spec!(
    /// Puts the files selected on the clipboard, to be copied.
    COPY, CopySelected, "file.copy", "Copy", ["ctrl-c"]
);
spec!(
    /// Puts the files selected on the clipboard, to be moved.
    CUT, CutSelected, "file.cut", "Cut", ["ctrl-x"]
);
spec!(
    /// Copies or moves the files on the clipboard into the folder on
    /// screen.
    PASTE, Paste, "file.paste", "Paste", ["ctrl-v"]
);
spec!(
    TRASH,
    TrashSelected,
    "file.trash",
    "Move to Trash",
    ["delete"]
);
spec!(
    /// Renames the one entry selected, in place.
    RENAME, RenameSelected, "file.rename", "Rename", ["f2"]
);
spec!(
    /// Makes a new folder in the folder on screen and renames it.
    NEW_FOLDER, NewFolder, "file.new-folder", "New folder", ["ctrl-shift-n"]
);
spec!(
    /// Undoes what the island's note names, or else the window's last
    /// job that can still be undone.
    UNDO, Undo, "edit.undo", "Undo", ["ctrl-z"]
);
spec!(
    /// Opens the quick look on the selected entry, or closes it.
    PREVIEW, TogglePreview, "preview.toggle", "Preview", ["space"]
);
spec!(
    /// Shows the entry before in the quick look.
    PREVIEW_PREVIOUS, PreviewPrevious, "preview.previous", "Previous", ["left"]
);
spec!(
    /// Shows the entry after in the quick look.
    PREVIEW_NEXT, PreviewNext, "preview.next", "Next", ["right"]
);
spec!(
    /// Puts the caret in the search box. `/` does too, unless a field
    /// has the keys (see `view`).
    FIND,
    FocusSearch,
    "search.focus",
    "Search",
    ["ctrl-f"]
);

spec!(
    /// Puts the entries selected in the trash back where they were.
    RESTORE, RestoreSelected, "file.restore", "Restore", []
);
spec!(
    /// Asks, then deletes everything in the trash for good.
    EMPTY_TRASH, EmptyTrash, "file.empty-trash", "Empty Trash", []
);

spec!(
    /// Opens a new tab on the Start page.
    NEW_TAB, NewTab, "tab.new", "New tab", ["ctrl-t"]
);
spec!(
    /// Closes the tab on screen, unless it is the last one.
    CLOSE_TAB, CloseTab, "tab.close", "Close tab", ["ctrl-w"]
);
spec!(
    /// Shows the next tab, the first after the last.
    NEXT_TAB, NextTab, "tab.next", "Next tab", ["ctrl-tab"]
);
spec!(
    /// Shows the tab before, the last before the first.
    PREVIOUS_TAB, PreviousTab, "tab.previous", "Previous tab", ["ctrl-shift-tab"]
);
spec!(
    /// Lists the hidden entries in every folder, or leaves them out again.
    HIDDEN, ToggleHidden, "view.hidden", "Show hidden files", ["ctrl-h"]
);

spec!(
    /// Opens the right-click menu of the entries selected, or of the
    /// empty space when none are.
    MENU, OpenMenu, "menu.open", "Menu", ["menu", "shift-f10"]
);

/// Binds every action's default keys, and the keys of an open menu.
pub fn bind_keys(cx: &mut App) {
    crate::menu::bind_keys(cx);
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
                assert!(binding.action().partial_eq(&*spec.action()), "{key}");
            }
        }
    }

    #[test]
    fn each_row_sends_its_own_action() {
        let names: HashSet<_> = ACTIONS.iter().map(|spec| spec.action().name()).collect();
        assert_eq!(names.len(), ACTIONS.len());
        assert_eq!(COPY, COPY);
        assert_ne!(COPY, CUT);
        assert_eq!(format!("{COPY:?}"), "file.copy");
    }
}
