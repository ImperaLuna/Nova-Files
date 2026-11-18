//! A small menu that opens where the pointer was right-clicked: a file's
//! actions, a pinned place's "Remove from Places", a drive's Eject, or
//! the folders Games or Projects gather from. The
//! window holds at most one; it closes on a choice, on Escape, or on a
//! press outside it. Drawing is in `view/menu`.
//!
//! Each item names what it does as a [`MenuAction`], which the window
//! carries out once the item is chosen, so a menu holds no closures and
//! a test can read what it offers. An item that runs one of the window's
//! actions ([`MenuAction::Run`]) sends that action, exactly as its key
//! does, so a menu and a key can't drift apart; the menu shows the key
//! that starts it (see [`key_label`]).
//!
//! Items come in groups with a line between each. While it is open the
//! menu holds the keys: Up and Down move along it (round from one end to
//! the other), Enter chooses, Escape closes. The pointer moves the same
//! mark the keys do.

use std::path::PathBuf;

use gpui::{
    App, Context, FocusHandle, KeyBinding, KeybindingKeystroke, Pixels, Point, SharedString, Window,
};

use nova_files_api::{DriveAction, DriveId, Gathering};

use crate::assets::Icon;
use crate::commands::ActionSpec;
use crate::drives::ShownDrive;
use crate::window::MainWindow;

/// The key context an open menu sets.
pub const KEY_CONTEXT: &str = "Menu";

#[expect(
    clippy::derive_partial_eq_without_eq,
    reason = "GPUI's actions! macro derives PartialEq only"
)]
mod actions {
    gpui::actions!(nova_files_menu, [MenuUp, MenuDown, MenuChoose, MenuClose]);
}

pub use actions::*;

/// The menu on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    /// Where its top left corner sits, in window coordinates.
    pub at: Point<Pixels>,
    pub items: Vec<MenuItem>,
    /// The items a line is drawn above, by index.
    pub lines: Vec<usize>,
    /// The item the keys or the pointer are on.
    pub active: Option<usize>,
    /// Holds the keys while the menu is open.
    pub focus: FocusHandle,
    /// Whether the menu still has to take the keys, at the next frame.
    wants_focus: bool,
}

/// One item of a [`Menu`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub icon: Icon,
    pub label: SharedString,
    pub action: MenuAction,
    pub look: Look,
    /// A word on it at its right, where an action's keys go.
    pub detail: Option<SharedString>,
}

/// How an item stands out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Look {
    #[default]
    Plain,
    /// A choice that is on: strong, its icon orange.
    On,
    /// It removes something: drawn in the error color.
    Danger,
}

/// What a menu item does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    /// Take a pinned folder out of Places.
    Unpin(PathBuf),
    /// Open a drive, mounting it first when needed.
    OpenDrive(ShownDrive),
    /// Open a drive in a new tab, mounting it first when needed.
    OpenDriveInNewTab(ShownDrive),
    /// Make a drive safe to unplug.
    Eject(DriveId),
    /// Open a folder in a new tab.
    OpenInNewTab(PathBuf),
    /// Send one of the window's actions, as its key does.
    Run(&'static ActionSpec),
    /// Stop gathering a place from a folder.
    Ungather(Gathering, PathBuf),
    /// Pick a folder to add to a place.
    AddGathered(Gathering),
}

impl Menu {
    /// Whether item `index` is the one marked.
    pub fn marks(&self, index: usize) -> bool {
        self.active == Some(index)
    }
}

impl MenuItem {
    /// A plain item.
    pub fn new(icon: Icon, label: impl Into<SharedString>, action: MenuAction) -> Self {
        Self {
            icon,
            label: label.into(),
            action,
            look: Look::Plain,
            detail: None,
        }
    }

    /// An item that sends `spec`'s action, named by its label.
    pub fn run(icon: Icon, spec: &'static ActionSpec) -> Self {
        Self::new(icon, spec.label, MenuAction::Run(spec))
    }

    /// This item, with `detail` at its right.
    #[must_use]
    pub fn with_detail(mut self, detail: Option<SharedString>) -> Self {
        self.detail = detail;
        self
    }

    /// This item, standing out as `look` says.
    #[must_use]
    pub const fn with_look(mut self, look: Look) -> Self {
        self.look = look;
        self
    }
}

impl MainWindow {
    /// Opens a menu of `items` at `at`, in place of any menu open.
    pub(crate) fn open_menu(
        &mut self,
        at: Point<Pixels>,
        items: Vec<MenuItem>,
        cx: &mut Context<Self>,
    ) {
        self.open_menu_groups(at, vec![items], false, cx);
    }

    /// Opens a menu of `groups`, with a line between each, at `at`, in
    /// place of any menu open. Empty groups are left out, and so is a menu
    /// with no items. Opened `by_key`, its first item is marked.
    pub(crate) fn open_menu_groups(
        &mut self,
        at: Point<Pixels>,
        groups: Vec<Vec<MenuItem>>,
        by_key: bool,
        cx: &mut Context<Self>,
    ) {
        let mut items = Vec::new();
        let mut lines = Vec::new();
        for group in groups.into_iter().filter(|group| !group.is_empty()) {
            if !items.is_empty() {
                lines.push(items.len());
            }
            items.extend(group);
        }
        self.menu = (!items.is_empty()).then(|| Menu {
            at,
            items,
            lines,
            active: by_key.then_some(0),
            focus: cx.focus_handle(),
            wants_focus: true,
        });
        cx.notify();
    }

    /// Closes the menu, if one is open.
    pub(crate) fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    /// Gives the keys to a menu that just opened.
    pub(crate) fn focus_menu(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(menu) = self.menu.as_mut().filter(|menu| menu.wants_focus) {
            menu.wants_focus = false;
            window.focus(&menu.focus, cx);
        }
    }

    /// Moves the menu's mark to the next item, or with `back` the one
    /// before, round from one end to the other.
    pub(crate) fn step_menu(&mut self, back: bool, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        let count = menu.items.len();
        menu.active = Some(match (menu.active, back) {
            (None, false) => 0,
            (None, true) => count.saturating_sub(1),
            (Some(index), false) => (index + 1) % count,
            (Some(index), true) => (index + count - 1) % count,
        });
        cx.notify();
    }

    /// Marks item `index`, under the pointer.
    pub(crate) fn mark_menu_item(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(menu) = self.menu.as_mut()
            && menu.active != Some(index)
        {
            menu.active = Some(index);
            cx.notify();
        }
    }

    /// Carries out the marked item, if any.
    pub(crate) fn choose_marked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.menu.as_ref().and_then(|menu| menu.active) {
            self.choose(index, window, cx);
        }
    }

    /// Carries out item `index` of the menu on screen and closes it. The
    /// keys go back to the window first, so an action it sends lands
    /// where its key would.
    pub(crate) fn choose(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self
            .menu
            .take()
            .and_then(|menu| menu.items.into_iter().nth(index))
        else {
            return;
        };
        window.focus(&self.keys_home(cx), cx);
        match item.action {
            MenuAction::Unpin(path) => self.unpin(path),
            MenuAction::OpenDrive(drive) => self.open_drive(drive),
            MenuAction::OpenDriveInNewTab(drive) => self.open_drive_in_new_tab(drive, cx),
            MenuAction::OpenInNewTab(folder) => self.open_in_new_tab(folder, cx),
            MenuAction::Eject(drive) => self.act_on_drive(DriveAction::Eject(drive)),
            MenuAction::Run(spec) => window.dispatch_action(spec.action(), cx),
            MenuAction::Ungather(place, path) => self.ungather(place, path, cx),
            MenuAction::AddGathered(place) => self.pick_folder_to_gather(place, cx),
        }
        cx.notify();
    }

    /// The menu on screen, if any.
    pub const fn menu(&self) -> Option<&Menu> {
        self.menu.as_ref()
    }
}

/// Binds the keys an open menu takes.
pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", MenuUp, context),
        KeyBinding::new("down", MenuDown, context),
        KeyBinding::new("enter", MenuChoose, context),
        KeyBinding::new("escape", MenuClose, context),
    ]);
}

/// How a menu shows the keys of `binding`: each key named as it reads on
/// a keyboard, modifiers first (`Ctrl Shift N`, `Del`, `F2`).
pub fn key_label(binding: &KeyBinding) -> Option<SharedString> {
    let keys: Vec<String> = binding.keystrokes().iter().map(keystroke_label).collect();
    (!keys.is_empty()).then(|| keys.join(KEY_GAP).into())
}

/// Between the words of a key label.
const KEY_GAP: &str = " ";

/// One keystroke's label.
fn keystroke_label(keystroke: &KeybindingKeystroke) -> String {
    let modifiers = keystroke.modifiers();
    let held = [
        (modifiers.control, "Ctrl"),
        (modifiers.alt, "Alt"),
        (modifiers.shift, "Shift"),
        (modifiers.platform, "Super"),
    ];
    held.into_iter()
        .filter(|(down, _)| *down)
        .map(|(_, name)| name.to_owned())
        .chain([key_name(keystroke.key())])
        .collect::<Vec<_>>()
        .join(KEY_GAP)
}

/// A key's name as printed on the key: GPUI's name with its first letter
/// a capital, and the long names keyboards shorten.
fn key_name(key: &str) -> String {
    let short = match key {
        "delete" => "Del",
        "escape" => "Esc",
        "pageup" => "PgUp",
        "pagedown" => "PgDn",
        other => other,
    };
    let mut letters = short.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{COPY, NEW_FOLDER, RENAME, TRASH};

    fn label(spec: &ActionSpec) -> Option<String> {
        key_label(&spec.binding(spec.keys[0])).map(String::from)
    }

    #[test]
    fn keys_read_as_printed_on_the_keyboard() {
        assert_eq!(label(&COPY).as_deref(), Some("Ctrl C"));
        assert_eq!(label(&NEW_FOLDER).as_deref(), Some("Ctrl Shift N"));
        assert_eq!(label(&TRASH).as_deref(), Some("Del"));
        assert_eq!(label(&RENAME).as_deref(), Some("F2"));
        assert_eq!(key_name("space"), "Space");
        assert_eq!(key_name("escape"), "Esc");
        assert_eq!(key_name("pageup"), "PgUp");
        assert_eq!(key_name("pagedown"), "PgDn");
        assert_eq!(key_name(""), "");
    }

    #[test]
    fn every_modifier_is_named_in_order() {
        let binding = KeyBinding::new("ctrl-alt-shift-super-x", MenuUp, None);
        assert_eq!(key_label(&binding).unwrap(), "Ctrl Alt Shift Super X");
        let chord = KeyBinding::new("alt-left enter", MenuUp, None);
        assert_eq!(key_label(&chord).unwrap(), "Alt Left Enter");
    }

    #[test]
    fn a_run_item_is_named_by_its_action() {
        let item = MenuItem::run(Icon::Copy, &COPY);
        assert_eq!(item.label, "Copy");
        assert_eq!(item.action, MenuAction::Run(&COPY));
        assert_eq!(item.look, Look::Plain);
        assert_eq!(item.with_look(Look::Danger).look, Look::Danger);
    }
}
