//! Naming a drop before it lands: while files dragged out of a list are
//! over a target, the chip under the pointer says what dropping them there
//! does, and where (see `dropping::label`): "Move to Documents".
//!
//! The drag is aimed as the pointer moves, before the window draws again:
//! every target first hears the move and lets go of the aim, then the
//! targets under the pointer hear it, innermost first (a folder row before
//! the list it is in), and the first that takes these files aims the drag
//! at itself (see `view::drop`). That is the target a drop lands on, and
//! the one lit. Over no such target, the chip says what is dragged.
//!
//! Whether a drop no key picks moves or copies depends on the drives,
//! which only the core looks at: the first time the chip names a folder,
//! the window the drag started in asks the core (`Command::DropWould`),
//! and the answer stays with the drag until it ends, wherever it goes.
//! Until it comes, the chip says what is dragged.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use gpui::{Context, SharedString};
use nova_files_api::{Command, DropAskId, DropKind};

use super::MainWindow;
use super::dragging::Drag;
use crate::dropping::DropAt;
use crate::places::TRASH_LABEL;

/// A target files are over: where they would drop, and its name.
#[derive(Debug, Clone)]
pub struct Aim {
    pub at: DropAt,
    pub name: SharedString,
}

/// What a drag knows about where it is aimed.
#[derive(Debug, Default)]
pub struct Aims {
    /// The target under the pointer, as of the last move.
    aim: RefCell<Option<Aim>>,
    /// What a drop no key picks does in each folder asked about; `None`
    /// while the core hasn't said.
    by_drive: RefCell<HashMap<PathBuf, Option<DropKind>>>,
}

impl Aim {
    /// The folder `path`, named `name`.
    pub fn folder(path: PathBuf, name: impl Into<SharedString>) -> Self {
        Self {
            at: DropAt::Folder(path),
            name: name.into(),
        }
    }

    /// The trash.
    pub fn trash() -> Self {
        Self {
            at: DropAt::Trash,
            name: TRASH_LABEL.into(),
        }
    }
}

impl Aims {
    /// Aims the drag at `aim`, unless a target under the pointer already
    /// took it on this move.
    pub fn aim(&self, aim: Aim) {
        self.aim.borrow_mut().get_or_insert(aim);
    }

    /// Lets go of the aim, as the pointer moves.
    pub fn let_go(&self) {
        self.aim.borrow_mut().take();
    }

    /// The target the drag is aimed at.
    pub fn aimed(&self) -> Option<Aim> {
        self.aim.borrow().clone()
    }

    /// What the core said a drop into `into` with no key held does.
    pub fn by_drive(&self, into: &Path) -> Option<DropKind> {
        self.by_drive.borrow().get(into).copied().flatten()
    }

    /// Whether `into` hasn't been asked about yet; it counts as asked
    /// from now on.
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

/// The window's asks of what a drop would do, waiting for their answers,
/// and the drag of files over it.
#[derive(Debug, Default)]
pub struct Asks {
    last: u64,
    /// Each ask's drag, while it lasts, and the folder asked about.
    waiting: HashMap<DropAskId, (Weak<Drag>, PathBuf)>,
    /// The drag of files from a list that last moved over the window.
    over: Weak<Drag>,
}

impl MainWindow {
    /// Files dragged from a list, `drag`, moved: over the window, and no
    /// longer aimed anywhere until a target under the pointer says.
    pub(crate) fn drag_moved(&mut self, drag: &Rc<Drag>) {
        drag.aims.let_go();
        self.drop_asks.over = Rc::downgrade(drag);
    }

    /// What the chip of the drag over the window says of its drop with
    /// `modifiers` held; none away from targets, or before the core said.
    pub fn drop_says(&self, modifiers: gpui::Modifiers) -> Option<String> {
        self.drag_over()?.says(modifiers)
    }

    /// The drag of files from a list moving over the window, while it
    /// lasts.
    pub(crate) fn drag_over(&self) -> Option<Rc<Drag>> {
        self.drop_asks.over.upgrade()
    }

    /// Asks the core what dropping `drag`'s files into `into` does when
    /// no key picks.
    pub(crate) fn ask_drop_kind(&mut self, drag: &Rc<Drag>, into: PathBuf) {
        let (Some(connection), Some(sources)) = (&self.connection, drag.sources()) else {
            return;
        };
        self.drop_asks.last += 1;
        let id = DropAskId(self.drop_asks.last);
        // A core gone ends the window's events, which shows it.
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

    /// Ask `id` is answered: the drag, if it is still on, keeps `kind`
    /// and its chip shows it, in whichever window it is.
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
            // The drag may be over another of the app's windows by now.
            cx.refresh_windows();
        }
    }
}
