use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use gpui::{Context, SharedString};
use nova_files_api::{Command, DropAskId, DropKind};
use nova_files_gathered_api::Gathering;

use super::MainWindow;
use super::dragging::Drag;
use crate::dropping::DropAt;
use crate::places::{self, TRASH_LABEL};

#[derive(Debug, Clone)]
pub struct Aim {
    pub at: DropAt,
    pub name: SharedString,
}

#[derive(Debug, Default)]
pub struct Aims {
    aim: RefCell<Option<Aim>>,
    by_drive: RefCell<HashMap<PathBuf, Option<DropKind>>>,
}

impl Aim {
    pub fn folder(path: PathBuf, name: impl Into<SharedString>) -> Self {
        Self {
            at: DropAt::Folder(path),
            name: name.into(),
        }
    }

    pub fn gathered(place: Gathering) -> Self {
        Self {
            at: DropAt::Gathered(place),
            name: places::gathering_look(place).0.into(),
        }
    }

    pub fn trash() -> Self {
        Self {
            at: DropAt::Trash,
            name: TRASH_LABEL.into(),
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

    pub fn by_drive(&self, into: &Path) -> Option<DropKind> {
        self.by_drive.borrow().get(into).copied().flatten()
    }

    pub fn first_ask(&self, into: &Path) -> bool {
        let mut asked = self.by_drive.borrow_mut();
        if asked.contains_key(into) {
            return false;
        }
        asked.insert(into.to_path_buf(), None);
        true
    }

    fn answered(&self, into: PathBuf, kind: DropKind) {
        self.by_drive.borrow_mut().insert(into, Some(kind));
    }
}

#[derive(Debug, Default)]
pub struct Asks {
    last: u64,
    waiting: HashMap<DropAskId, (Weak<Drag>, PathBuf)>,
    over: Weak<Drag>,
}

impl Asks {
    const fn next_id(&mut self) -> DropAskId {
        self.last += 1;
        DropAskId(self.last)
    }
}

impl MainWindow {
    pub(crate) fn drag_moved(&mut self, drag: &Rc<Drag>) {
        drag.aims.let_go();
        self.drop_asks.over = Rc::downgrade(drag);
    }

    pub fn drop_says(&self, modifiers: gpui::Modifiers) -> Option<String> {
        self.drag_over()?.says(modifiers)
    }

    pub(crate) fn drag_over(&self) -> Option<Rc<Drag>> {
        self.drop_asks.over.upgrade()
    }

    pub(crate) fn ask_drop_kind(&mut self, drag: &Rc<Drag>, into: PathBuf) {
        let (Some(connection), Some(sources)) = (&self.connection, drag.sources()) else {
            return;
        };
        let id = self.drop_asks.next_id();
        let asked = connection.send(Command::DropWould {
            id,
            sources: sources.paths.clone(),
            into: into.clone(),
        });
        if asked.is_ok() {
            self.drop_asks
                .waiting
                .insert(id, (Rc::downgrade(drag), into));
        }
    }

    pub(super) fn drop_would_arrived(
        &mut self,
        id: DropAskId,
        kind: DropKind,
        cx: &mut Context<Self>,
    ) {
        let Some((drag, into)) = self.drop_asks.waiting.remove(&id) else {
            return;
        };
        if let Some(drag) = drag.upgrade() {
            drag.aims.answered(into, kind);
            cx.notify();
            cx.refresh_windows();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_ask_has_an_id_of_its_own() {
        let mut asks = Asks::default();
        let first = asks.next_id();
        assert_ne!(asks.next_id(), first);
    }
}
