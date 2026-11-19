use std::fmt;
use std::rc::Rc;

use gpui::{App, SharedString};

use crate::assets::Icon;
use crate::command::CommandSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuAt {
    Entry,
    Sheet,
    Island,
    PathBarEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MenuGroup {
    Open,
    Edit,
    Select,
    View,
    Show,
    Danger,
    Info,
    Part,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Look {
    #[default]
    Plain,
    On,
    Danger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub icon: Icon,
    pub label: SharedString,
    pub run: &'static CommandSpec,
    pub look: Look,
    pub detail: Option<SharedString>,
    pub selector: Option<&'static str>,
    pub icon_only: bool,
}

impl MenuItem {
    pub fn run(icon: Icon, spec: &'static CommandSpec) -> Self {
        Self {
            icon,
            label: spec.label.into(),
            run: spec,
            look: Look::Plain,
            detail: None,
            selector: None,
            icon_only: false,
        }
    }

    #[must_use]
    pub fn with_label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    #[must_use]
    pub const fn with_look(mut self, look: Look) -> Self {
        self.look = look;
        self
    }

    #[must_use]
    pub fn with_detail(mut self, detail: Option<SharedString>) -> Self {
        self.detail = detail;
        self
    }

    #[must_use]
    pub const fn with_selector(mut self, selector: &'static str) -> Self {
        self.selector = Some(selector);
        self
    }

    #[must_use]
    pub const fn icon_only(mut self) -> Self {
        self.icon_only = true;
        self
    }

    pub fn selector(&self) -> &'static str {
        self.selector.unwrap_or(self.run.id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub group: MenuGroup,
    pub rank: u8,
    pub items: Vec<MenuItem>,
}

impl Placed {
    pub const FIRST: u8 = 0;
    pub const SECOND: u8 = 1;
    pub const LAST: u8 = u8::MAX;

    pub const fn new(group: MenuGroup, rank: u8, items: Vec<MenuItem>) -> Self {
        Self { group, rank, items }
    }
}

pub fn arrange(mut placed: Vec<Placed>) -> Vec<Vec<MenuItem>> {
    placed.sort_by_key(|placed| (placed.group, placed.rank));
    let mut groups: Vec<(MenuGroup, Vec<MenuItem>)> = Vec::new();
    for Placed { group, items, .. } in placed {
        if items.is_empty() {
            continue;
        }
        match groups.last_mut() {
            Some((last, joined)) if *last == group && group != MenuGroup::Part => {
                joined.extend(items);
            }
            _ => groups.push((group, items)),
        }
    }
    groups.into_iter().map(|(_, items)| items).collect()
}

pub type OnRun = Rc<dyn Fn(&mut App)>;

#[derive(Clone)]
pub struct RowItem {
    pub icon: Icon,
    pub label: SharedString,
    pub look: Look,
    pub run: OnRun,
}

impl RowItem {
    pub fn new(
        icon: Icon,
        label: impl Into<SharedString>,
        run: impl Fn(&mut App) + 'static,
    ) -> Self {
        Self {
            icon,
            label: label.into(),
            look: Look::Plain,
            run: Rc::new(run),
        }
    }

    #[must_use]
    pub const fn with_look(mut self, look: Look) -> Self {
        self.look = look;
        self
    }
}

impl fmt::Debug for RowItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

#[derive(Debug, Clone)]
pub struct SidebarRow {
    pub id: SharedString,
    pub icon: Icon,
    pub name: SharedString,
    pub number: Option<SharedString>,
    pub number_look: Look,
    pub opens: crate::Location,
    pub here: bool,
    pub depth: u8,
    pub expander: Option<Expander>,
    pub button: Option<RowItem>,
    pub rank: u8,
}

#[derive(Debug, Clone)]
pub struct Expander {
    pub open: bool,
    pub toggle: RowItem,
}

impl SidebarRow {
    pub fn new(
        id: impl Into<SharedString>,
        icon: Icon,
        name: impl Into<SharedString>,
        opens: crate::Location,
    ) -> Self {
        Self {
            id: id.into(),
            icon,
            name: name.into(),
            number: None,
            number_look: Look::Plain,
            opens,
            here: false,
            depth: 0,
            expander: None,
            button: None,
            rank: Self::LAST,
        }
    }

    pub const FIRST: u8 = 0;
    pub const START: u8 = 10;
    pub const OWN: u8 = 20;
    pub const GATHERED: u8 = 30;
    pub const LAST: u8 = u8::MAX;

    #[must_use]
    pub const fn rank(mut self, rank: u8) -> Self {
        self.rank = rank;
        self
    }

    #[must_use]
    pub fn number(mut self, number: Option<SharedString>) -> Self {
        self.number = number;
        self
    }

    #[must_use]
    pub const fn number_look(mut self, look: Look) -> Self {
        self.number_look = look;
        self
    }

    #[must_use]
    pub const fn here(mut self, here: bool) -> Self {
        self.here = here;
        self
    }

    #[must_use]
    pub const fn depth(mut self, depth: u8) -> Self {
        self.depth = depth;
        self
    }

    #[must_use]
    pub fn expander(mut self, open: bool, toggle: RowItem) -> Self {
        self.expander = Some(Expander { open, toggle });
        self
    }

    #[must_use]
    pub fn button(mut self, button: RowItem) -> Self {
        self.button = Some(button);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SidebarAt {
    Places,
    Drives,
    Tree,
}

#[cfg(test)]
mod tests {
    use super::*;

    mod actions {
        #![expect(
            clippy::derive_partial_eq_without_eq,
            reason = "GPUI's actions! macro derives PartialEq only"
        )]
        gpui::actions!(menu_test, [Pick]);
    }
    use actions::Pick;

    crate::command_spec!(PICK, Pick, "test.pick", "Pick", []);

    fn item(label: &'static str) -> MenuItem {
        MenuItem::run(Icon::Open, &PICK).with_label(label)
    }

    fn labels(groups: &[Vec<MenuItem>]) -> Vec<Vec<&str>> {
        groups
            .iter()
            .map(|items| items.iter().map(|item| item.label.as_ref()).collect())
            .collect()
    }

    #[test]
    fn groups_come_in_their_order_and_join_by_rank() {
        let groups = arrange(vec![
            Placed::new(MenuGroup::Info, Placed::FIRST, vec![item("info")]),
            Placed::new(MenuGroup::Edit, Placed::SECOND, vec![item("rename")]),
            Placed::new(MenuGroup::Open, Placed::FIRST, vec![item("open")]),
            Placed::new(
                MenuGroup::Edit,
                Placed::FIRST,
                vec![item("cut"), item("copy")],
            ),
        ]);

        assert_eq!(
            labels(&groups),
            [vec!["open"], vec!["cut", "copy", "rename"], vec!["info"]]
        );
    }

    #[test]
    fn each_part_group_stands_alone_and_empty_groups_go() {
        let groups = arrange(vec![
            Placed::new(MenuGroup::Part, Placed::LAST, vec![item("select all")]),
            Placed::new(MenuGroup::Part, Placed::FIRST, vec![item("restore")]),
            Placed::new(MenuGroup::Part, Placed::FIRST, vec![item("empty")]),
            Placed::new(MenuGroup::Edit, Placed::FIRST, Vec::new()),
        ]);

        assert_eq!(
            labels(&groups),
            [vec!["restore"], vec!["empty"], vec!["select all"]]
        );
    }
}
