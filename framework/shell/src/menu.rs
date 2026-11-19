use gpui::SharedString;

use crate::assets::Icon;
use crate::command::CommandSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuAt {
    Entry,
    Sheet,
    Island,
    PathBarEnd,
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

    pub fn selector(&self) -> &'static str {
        self.selector.unwrap_or(self.run.id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarRow {
    pub id: &'static str,
    pub icon: Icon,
    pub name: SharedString,
    pub number: Option<SharedString>,
    pub opens: crate::Location,
    pub here: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SidebarAt {
    Places,
}
