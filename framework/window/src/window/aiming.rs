use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use gpui::SharedString;

use super::MainWindow;
use super::dragging::Drag;
use crate::dropping::DropAt;
use nova_files_shell::{Gap, PageRef};

#[derive(Debug, Clone)]
pub struct Aim {
    pub at: DropAt,
    pub name: SharedString,
}

#[derive(Debug, Default)]
pub struct Aims {
    aim: RefCell<Option<Aim>>,
}

impl Aim {
    pub fn folder(path: PathBuf, name: impl Into<SharedString>) -> Self {
        Self {
            at: DropAt::Folder(path),
            name: name.into(),
        }
    }

    pub fn gap(gap: Gap) -> Self {
        Self {
            name: gap.label.clone(),
            at: DropAt::Gap(gap),
        }
    }

    pub fn page(page: PageRef) -> Self {
        Self {
            name: page.title(),
            at: DropAt::Page(page),
        }
    }
}

impl Aims {
    pub fn aim(&self, aim: Aim) {
        self.aim.borrow_mut().get_or_insert(aim);
    }

    pub fn let_go(&self) {
        self.aim.borrow_mut().take();
    }

    pub fn aimed(&self) -> Option<Aim> {
        self.aim.borrow().clone()
    }
}

#[derive(Debug, Default)]
pub struct Asks {
    over: Weak<Drag>,
}

impl MainWindow {
    pub(crate) fn drag_moved(&mut self, drag: &Rc<Drag>) {
        drag.aims.let_go();
        self.drop_asks.over = Rc::downgrade(drag);
    }

    pub fn drop_says(&self, modifiers: gpui::Modifiers, cx: &gpui::App) -> Option<String> {
        let says = self.drag_over()?.says(modifiers, &self.parts, cx)?;
        Some(says.to_string())
    }

    pub fn drag_out(&self) -> Option<gpui::ExternalDragPayload> {
        self.drag_over()?.outward()
    }

    pub(crate) fn drag_over(&self) -> Option<Rc<Drag>> {
        self.drop_asks.over.upgrade()
    }
}
