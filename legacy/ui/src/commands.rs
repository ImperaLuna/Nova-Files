use gpui::App;
pub use nova_files_shell::CommandSpec;
use nova_files_shell::command_spec as spec;

pub use nova_files_shell::WINDOW_KEYS as KEY_CONTEXT;

pub use nova_files_shell::actions::*;

pub const ACTIONS: &[CommandSpec] = &[
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
    SELECT_LEFT,
    SELECT_RIGHT,
    SELECT_ALL,
    TOGGLE_SELECTED,
    CLEAR_SELECTION,
    TRASH,
    RENAME,
    NEW_FOLDER,
    UNDO,
    PREVIEW,
    PREVIEW_PREVIOUS,
    PREVIEW_NEXT,
    FIND,
    NEW_TAB,
    CLOSE_TAB,
    NEXT_TAB,
    PREVIOUS_TAB,
    HIDDEN,
    DETAILS,
    ICONS,
    GALLERY,
    BIGGER_ICONS,
    SMALLER_ICONS,
    MENU,
    PROPERTIES,
];

spec!(UP, GoUp, "go.up", "Up", ["alt-up"]);
spec!(BACK, GoBack, "go.back", "Back", ["alt-left", "backspace"]);
spec!(FORWARD, GoForward, "go.forward", "Forward", ["alt-right"]);
spec!(OPEN, OpenSelected, "entry.open", "Open", ["enter"]);
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
    SELECT_LEFT,
    SelectLeft,
    "select.left",
    "Select left",
    ["shift-left"]
);
spec!(
    SELECT_RIGHT,
    SelectRight,
    "select.right",
    "Select right",
    ["shift-right"]
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
    TOGGLE_SELECTED,
    ToggleSelected,
    "select.toggle",
    "Select or unselect",
    ["ctrl-space"]
);
spec!(
    CLEAR_SELECTION,
    ClearSelection,
    "select.none",
    "Select none",
    ["escape"]
);

spec!(
    TRASH,
    TrashSelected,
    "file.trash",
    "Move to Trash",
    ["delete"]
);
spec!(RENAME, RenameSelected, "file.rename", "Rename", ["f2"]);
spec!(
    NEW_FOLDER,
    NewFolder,
    "file.new-folder",
    "New folder",
    ["ctrl-shift-n"]
);
spec!(UNDO, Undo, "edit.undo", "Undo", ["ctrl-z"]);
spec!(
    PREVIEW,
    TogglePreview,
    "preview.toggle",
    "Preview",
    ["space"]
);
spec!(
    PREVIEW_PREVIOUS,
    PreviewPrevious,
    "preview.previous",
    "Previous",
    ["left"]
);
spec!(PREVIEW_NEXT, PreviewNext, "preview.next", "Next", ["right"]);
spec!(FIND, FocusSearch, "search.focus", "Search", ["ctrl-f"]);

spec!(NEW_TAB, NewTab, "tab.new", "New tab", ["ctrl-t"]);
spec!(CLOSE_TAB, CloseTab, "tab.close", "Close tab", ["ctrl-w"]);
spec!(NEXT_TAB, NextTab, "tab.next", "Next tab", ["ctrl-tab"]);
spec!(
    PREVIOUS_TAB,
    PreviousTab,
    "tab.previous",
    "Previous tab",
    ["ctrl-shift-tab"]
);
spec!(
    HIDDEN,
    ToggleHidden,
    "view.hidden",
    "Show hidden files",
    ["ctrl-h"]
);

spec!(DETAILS, ShowDetails, "view.details", "Details", ["ctrl-1"]);
spec!(ICONS, ShowIcons, "view.icons", "Icons", ["ctrl-2"]);
spec!(GALLERY, ShowGallery, "view.gallery", "Gallery", ["ctrl-3"]);
spec!(
    BIGGER_ICONS,
    BiggerIcons,
    "view.bigger",
    "Bigger icons",
    ["ctrl-=", "ctrl-+"]
);
spec!(
    SMALLER_ICONS,
    SmallerIcons,
    "view.smaller",
    "Smaller icons",
    ["ctrl--"]
);

spec!(MENU, OpenMenu, "menu.open", "Menu", ["menu", "shift-f10"]);

spec!(
    PROPERTIES,
    ShowProperties,
    "file.properties",
    "Properties",
    ["alt-enter"]
);

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

    use nova_files_clipboard_ui::{COPY, CUT};

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
