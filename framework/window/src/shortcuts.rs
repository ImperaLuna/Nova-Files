use gpui::{App, Entity, KeyBinding, ScrollHandle, SharedString, Subscription};

use crate::commands::CommandSpec;
use crate::menu::key_label;
use crate::text::TextField;

pub const KEY_CONTEXT: &str = "Shortcuts";

#[expect(
    clippy::derive_partial_eq_without_eq,
    reason = "GPUI's actions! macro derives PartialEq only"
)]
mod actions {
    gpui::actions!(nova_files_shortcuts, [ShortcutUp, ShortcutDown]);
}

pub use actions::*;

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", ShortcutUp, context),
        KeyBinding::new("down", ShortcutDown, context),
    ]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Go,
    Files,
    Folder,
    Select,
    Move,
    View,
    Preview,
    Tabs,
    More,
}

impl Group {
    pub fn of(spec: &CommandSpec) -> Self {
        match spec.id.split('.').next().unwrap_or_default() {
            "go" | "path" | "search" => Self::Go,
            "entry" | "file" | "edit" | "gathered" => Self::Files,
            "folder" => Self::Folder,
            "select" => Self::Select,
            "cursor" | "ring" => Self::Move,
            "view" | "help" => Self::View,
            "preview" => Self::Preview,
            "tab" | "window" | "menu" => Self::Tabs,
            _ => Self::More,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Go => "Go",
            Self::Files => "Files",
            Self::Folder => "This folder",
            Self::Select => "Select",
            Self::Move => "Move",
            Self::View => "View",
            Self::Preview => "Preview",
            Self::Tabs => "Tabs and windows",
            Self::More => "More",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    pub spec: &'static CommandSpec,
    pub group: Group,
    pub keys: Vec<SharedString>,
    pub runs: bool,
}

impl Shortcut {
    pub fn new(spec: &'static CommandSpec, runs: bool) -> Self {
        Self {
            spec,
            group: Group::of(spec),
            keys: spec
                .keys
                .iter()
                .filter_map(|keys| key_label(&spec.binding(keys)))
                .collect(),
            runs,
        }
    }

    fn fit(&self, words: &[String]) -> Option<Fit> {
        let typed = words.join(" ");
        if typed.is_empty() {
            return Some(Fit::Any);
        }
        let named = |keys: &&str| keys.replace('-', " ") == typed;
        if self.keys.iter().any(|keys| keys.to_lowercase() == typed)
            || self.spec.keys.iter().any(named)
        {
            return Some(Fit::Keys);
        }
        let name = self.spec.label.to_lowercase();
        let about = format!("{name} {}", self.group.label().to_lowercase());
        if !words.iter().all(|word| about.contains(word.as_str())) {
            return None;
        }
        let first = &words[0];
        Some(if name.starts_with(&typed) {
            Fit::Start
        } else if name.split(' ').any(|word| word.starts_with(first.as_str())) {
            Fit::WordStart
        } else if name.contains(first.as_str()) {
            Fit::Inside
        } else {
            Fit::Group
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Fit {
    Any,
    Keys,
    Start,
    WordStart,
    Inside,
    Group,
}

pub fn shown(shortcuts: &[Shortcut], typed: &str) -> Vec<usize> {
    let words: Vec<String> = typed.split_whitespace().map(str::to_lowercase).collect();
    let mut fits: Vec<(usize, Fit)> = shortcuts
        .iter()
        .enumerate()
        .filter_map(|(index, shortcut)| shortcut.fit(&words).map(|fit| (index, fit)))
        .collect();
    if words.is_empty() {
        fits.sort_by_key(|&(index, _)| (shortcuts[index].group, index));
    } else {
        fits.sort_by_key(|&(index, fit)| (!shortcuts[index].runs, fit, index));
    }
    fits.into_iter().map(|(index, _)| index).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picking {
    pub list: Vec<Shortcut>,
    pub shown: Vec<usize>,
    pub marked: Option<usize>,
    pub grouped: bool,
}

impl Picking {
    pub fn new(list: Vec<Shortcut>) -> Self {
        let mut picking = Self {
            list,
            shown: Vec::new(),
            marked: None,
            grouped: true,
        };
        picking.refilter("");
        picking
    }

    pub fn refilter(&mut self, typed: &str) {
        self.shown = shown(&self.list, typed);
        self.grouped = typed.trim().is_empty();
        let first = self.runnable().next();
        self.marked = first;
    }

    pub fn row(&self, at: usize) -> Option<&Shortcut> {
        self.shown.get(at).map(|&index| &self.list[index])
    }

    pub fn marked_spec(&self) -> Option<&'static CommandSpec> {
        self.marked.and_then(|at| self.row(at)).map(|row| row.spec)
    }

    pub fn step(&mut self, back: bool) {
        let runnable: Vec<usize> = self.runnable().collect();
        let count = runnable.len();
        if count == 0 {
            return;
        }
        let now = self
            .marked
            .and_then(|marked| runnable.iter().position(|&at| at == marked));
        let next = match (now, back) {
            (None, false) => 0,
            (None, true) => count - 1,
            (Some(now), false) => (now + 1) % count,
            (Some(now), true) => (now + count - 1) % count,
        };
        self.marked = Some(runnable[next]);
    }

    pub fn mark(&mut self, at: usize) -> bool {
        let runs = self.row(at).is_some_and(|row| row.runs);
        let changed = runs && self.marked != Some(at);
        if changed {
            self.marked = Some(at);
        }
        changed
    }

    fn runnable(&self) -> impl Iterator<Item = usize> + '_ {
        self.shown
            .iter()
            .enumerate()
            .filter(|&(_, &index)| self.list[index].runs)
            .map(|(at, _)| at)
    }
}

pub struct Shortcuts {
    pub picking: Picking,
    pub field: Entity<TextField>,
    pub scroll: ScrollHandle,
    pub(crate) _typing: Subscription,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{
        BACK, CLEAR_SELECTION, CLOSE_TAB, CURSOR_DOWN, CURSOR_PAGE_DOWN, FIND, PREVIEW_NEXT,
        RING_UP, SELECT_ALL, UP,
    };

    fn list() -> Vec<Shortcut> {
        vec![
            Shortcut::new(&SELECT_ALL, true),
            Shortcut::new(&BACK, true),
            Shortcut::new(&CLOSE_TAB, true),
            Shortcut::new(&UP, true),
            Shortcut::new(&PREVIEW_NEXT, false),
            Shortcut::new(&CURSOR_DOWN, true),
            Shortcut::new(&FIND, true),
            Shortcut::new(&RING_UP, true),
        ]
    }

    fn names(typed: &str) -> Vec<&'static str> {
        let list = list();
        shown(&list, typed)
            .into_iter()
            .map(|index| list[index].spec.id)
            .collect()
    }

    #[test]
    fn nothing_typed_lists_every_command_by_group() {
        assert_eq!(
            names(""),
            [
                "go.back",
                "go.up",
                "search.focus",
                "select.all",
                "cursor.down",
                "ring.up",
                "preview.next",
                "tab.close"
            ]
        );
    }

    #[test]
    fn a_key_typed_in_full_finds_its_command_first() {
        assert_eq!(names("ctrl a"), ["select.all"]);
        assert_eq!(names("Alt Left"), ["go.back"]);
        assert_eq!(names("backspace"), ["go.back"]);
        let list = [
            Shortcut::new(&CLEAR_SELECTION, true),
            Shortcut::new(&CURSOR_PAGE_DOWN, true),
        ];
        for (typed, found) in [("escape", 0), ("esc", 0), ("pagedown", 1), ("pgdn", 1)] {
            assert_eq!(shown(&list, typed), [found], "{typed}");
        }
    }

    #[test]
    fn names_that_start_with_the_words_come_before_names_that_hold_them() {
        assert_eq!(names("up"), ["go.up", "ring.up"]);
        assert_eq!(
            names("se"),
            ["select.all", "search.focus", "ring.up", "tab.close"]
        );
    }

    #[test]
    fn every_word_must_be_in_the_name_or_the_group() {
        assert_eq!(names("tab close"), ["tab.close"]);
        assert_eq!(names("move"), ["ring.up", "cursor.down"]);
        assert_eq!(names("zebra"), Vec::<&str>::new());
    }

    #[test]
    fn commands_that_cannot_run_come_last_and_are_never_marked() {
        assert_eq!(names("next"), ["cursor.down", "preview.next"]);
        let mut picking = Picking::new(vec![
            Shortcut::new(&CURSOR_DOWN, true),
            Shortcut::new(&PREVIEW_NEXT, false),
            Shortcut::new(&UP, true),
        ]);
        assert_eq!(picking.marked_spec(), Some(&UP));
        picking.step(false);
        assert_eq!(picking.marked_spec(), Some(&CURSOR_DOWN));
        picking.step(false);
        assert_eq!(picking.marked_spec(), Some(&UP));
        picking.step(true);
        assert_eq!(picking.marked_spec(), Some(&CURSOR_DOWN));
        assert!(!picking.mark(2));
        assert_eq!(picking.marked_spec(), Some(&CURSOR_DOWN));
        picking.refilter("zebra");
        assert_eq!(picking.marked_spec(), None);
        picking.step(false);
        assert_eq!(picking.marked_spec(), None);
    }

    #[test]
    fn keys_read_as_on_the_keyboard() {
        assert_eq!(Shortcut::new(&BACK, true).keys, ["Alt Left", "Backspace"]);
        assert_eq!(Group::of(&RING_UP).label(), "Move");
    }
}
