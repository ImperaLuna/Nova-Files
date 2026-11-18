use std::path::PathBuf;

use gpui::{
    App, Context, FocusHandle, KeyBinding, KeybindingKeystroke, Pixels, Point, SharedString, Window,
};

use nova_files_api::Gathering;
use nova_files_drives_api::{DriveAction, DriveId};

use crate::assets::Icon;
use crate::commands::ActionSpec;
use crate::drives::ShownDrive;
use crate::window::{AccessLevel, MainWindow, Who};

pub const KEY_CONTEXT: &str = "Menu";

#[expect(
    clippy::derive_partial_eq_without_eq,
    reason = "GPUI's actions! macro derives PartialEq only"
)]
mod actions {
    gpui::actions!(nova_files_menu, [MenuUp, MenuDown, MenuChoose, MenuClose]);
}

pub use actions::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    pub at: Point<Pixels>,
    pub items: Vec<MenuItem>,
    pub lines: Vec<usize>,
    pub active: Option<usize>,
    pub focus: FocusHandle,
    wants_focus: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub icon: Icon,
    pub label: SharedString,
    pub action: MenuAction,
    pub look: Look,
    pub detail: Option<SharedString>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Look {
    #[default]
    Plain,
    On,
    Danger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    Unpin(PathBuf),
    OpenDrive(ShownDrive),
    OpenDriveInNewTab(ShownDrive),
    Eject(DriveId),
    OpenInNewTab(PathBuf),
    SetDefaultApp { mime: String, app: String },
    SetAccess(Who, AccessLevel),
    Run(&'static ActionSpec),
    Ungather(Gathering, PathBuf),
    AddGathered(Gathering),
}

impl Menu {
    pub fn marks(&self, index: usize) -> bool {
        self.active == Some(index)
    }
}

impl MenuItem {
    pub fn new(icon: Icon, label: impl Into<SharedString>, action: MenuAction) -> Self {
        Self {
            icon,
            label: label.into(),
            action,
            look: Look::Plain,
            detail: None,
        }
    }

    pub fn run(icon: Icon, spec: &'static ActionSpec) -> Self {
        Self::new(icon, spec.label, MenuAction::Run(spec))
    }

    #[must_use]
    pub fn with_detail(mut self, detail: Option<SharedString>) -> Self {
        self.detail = detail;
        self
    }

    #[must_use]
    pub const fn with_look(mut self, look: Look) -> Self {
        self.look = look;
        self
    }
}

impl MainWindow {
    pub(crate) fn open_menu(
        &mut self,
        at: Point<Pixels>,
        items: Vec<MenuItem>,
        cx: &mut Context<Self>,
    ) {
        self.open_menu_groups(at, vec![items], false, cx);
    }

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

    pub(crate) fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn focus_menu(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(menu) = self.menu.as_mut().filter(|menu| menu.wants_focus) {
            menu.wants_focus = false;
            window.focus(&menu.focus, cx);
        }
    }

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

    pub(crate) fn mark_menu_item(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(menu) = self.menu.as_mut()
            && menu.active != Some(index)
        {
            menu.active = Some(index);
            cx.notify();
        }
    }

    pub(crate) fn choose_marked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.menu.as_ref().and_then(|menu| menu.active) {
            self.choose(index, window, cx);
        }
    }

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
            MenuAction::SetDefaultApp { mime, app } => self.set_default_app(mime, app),
            MenuAction::SetAccess(who, level) => self.set_access(who, level),
            MenuAction::Run(spec) => window.dispatch_action(spec.action(), cx),
            MenuAction::Ungather(place, path) => self.ungather(place, path, cx),
            MenuAction::AddGathered(place) => self.pick_folder_to_gather(place, cx),
        }
        cx.notify();
    }

    pub const fn menu(&self) -> Option<&Menu> {
        self.menu.as_ref()
    }
}

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", MenuUp, context),
        KeyBinding::new("down", MenuDown, context),
        KeyBinding::new("enter", MenuChoose, context),
        KeyBinding::new("escape", MenuClose, context),
    ]);
}

pub fn key_label(binding: &KeyBinding) -> Option<SharedString> {
    let keys: Vec<String> = binding.keystrokes().iter().map(keystroke_label).collect();
    (!keys.is_empty()).then(|| keys.join(KEY_GAP).into())
}

const KEY_GAP: &str = " ";

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
