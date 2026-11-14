//! Accepting what the pick bar says: Enter in the name box, or the Open or
//! Save button. A typed path is tried as a folder first and, if the core
//! can't list it, the window goes to the folder it is in with the last part
//! as the name (a `Jump`). A name in the folder on screen is opened,
//! picked or saved at; nothing typed picks the selection, or for folders
//! the folder on screen. When the folder a jump asked for answers or is
//! refused, the box and any pending save follow it here too. See `pick`
//! for the whole flow.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use gpui::{Context, Window};
use nova_files_api::{EntryId, EntryKind, ListingId};

use super::{Mode, typed};
use crate::window::{MainWindow, tidy};

/// A typed path the window asked the core for.
pub(super) struct Jump {
    /// Whether a refusal goes to the folder the path is in instead.
    fallback: bool,
    /// What the name box holds once the window is there.
    name: OsString,
    /// The listing it asked for; any other one means the user went
    /// somewhere else, which drops it.
    listing: Option<ListingId>,
}

impl MainWindow {
    /// Enter in the name box, or the Open or Save button: does what the
    /// box says (see the module's notes). A save onto a file that is there
    /// asks first; asked, Enter keeps both.
    pub fn accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.waiting() {
            return;
        }
        let Some(pick) = &self.pick else { return };
        if pick.conflict.is_some() {
            self.keep_both();
            cx.notify();
            return;
        }
        let folder = self.folder().unwrap_or(&self.home).to_path_buf();
        let text = pick.name.read(cx).text().to_owned();
        let clicked = pick
            .copied
            .as_ref()
            .filter(|(_, copied)| *copied == text)
            .map(|(ids, _)| self.shown_entries(ids))
            .filter(|found| !found.is_empty());
        if let Some(mut found) = clicked {
            let answered = if found.len() == 1 {
                found
                    .pop()
                    .is_some_and(|found| self.accept_found(&folder, found))
            } else {
                // Only files are copied into the box.
                let paths = found.into_iter().map(|(_, _, name)| folder.join(name));
                self.answer_all(paths.collect())
            };
            if answered {
                window.remove_window();
            }
            cx.notify();
            return;
        }
        // The app's own name, or one the window put there, unchanged, is a
        // name with its exact bytes, never a path to read.
        if let Some(suggested) = pick.exact_name(&text) {
            if self.accept_exact(&folder, suggested) {
                window.remove_window();
            }
            cx.notify();
            return;
        }
        let answered = match typed::read(&text, &self.home, &folder) {
            typed::Typed::Nothing => self.accept_selection(&folder),
            typed::Typed::Folder(path) => self.jump(&path, false),
            typed::Typed::Path(path) => self.jump(&path, true),
            typed::Typed::Name(name) => self.accept_name(&folder, &name),
        };
        if answered {
            window.remove_window();
        }
        cx.notify();
    }

    /// With nothing typed: picks the files or folders selected, or for
    /// folders the folder on screen. Whether it answered.
    fn accept_selection(&mut self, folder: &Path) -> bool {
        let Some(pick) = &self.pick else { return false };
        let mode = pick.mode;
        let wanted = match mode {
            Mode::Files => EntryKind::File,
            Mode::Folders => EntryKind::Folder,
            Mode::Save => return false,
        };
        let selected = self
            .selection
            .as_ref()
            .filter(|selection| self.listing_id() == Some(selection.listing))
            .map(|selection| self.shown_entries(selection.ids()))
            .unwrap_or_default();
        let paths: Vec<PathBuf> = selected
            .iter()
            .filter(|&&(kind, _, _)| kind == wanted)
            .map(|(_, _, name)| folder.join(name))
            .collect();
        match (mode, paths.is_empty()) {
            (_, false) => self.answer_all(paths),
            (Mode::Folders, true) => self.answer(folder.to_path_buf()),
            // A folder the keys moved to, in a picker for files: Enter
            // goes into it.
            (Mode::Files | Mode::Save, true) => {
                if let [(EntryKind::Folder, id, _)] = selected[..] {
                    self.open_entry(id);
                }
                false
            }
        }
    }

    /// `name` typed in the folder on screen: goes into it if it is a
    /// folder there; otherwise opens it as a file, or saves at it. Whether
    /// it answered.
    fn accept_name(&mut self, folder: &Path, name: &str) -> bool {
        let found = self.listing.as_ref().and_then(|listing| {
            // A name shown the same way as another, but not its bytes,
            // never stands in for the one typed.
            let entry = listing
                .entries
                .iter()
                .find(|entry| entry.name == name)
                .or_else(|| {
                    listing
                        .entries
                        .iter()
                        .find(|entry| entry.name.to_string_lossy() == name)
                })?;
            Some((entry.kind, entry.id, entry.name.clone()))
        });
        match found {
            Some(found) => self.accept_found(folder, found),
            None => self.save_at(folder, OsString::from(name)),
        }
    }

    /// The app's suggested `name`, unchanged: the entry with exactly that
    /// name, or a save under it. Whether it answered.
    fn accept_exact(&mut self, folder: &Path, name: OsString) -> bool {
        let found = self.listing.as_ref().and_then(|listing| {
            let entry = listing.entries.iter().find(|entry| entry.name == name)?;
            Some((entry.kind, entry.id, entry.name.clone()))
        });
        match found {
            Some(found) => self.accept_found(folder, found),
            None => self.save_at(folder, name),
        }
    }

    /// A save under `name`, free in the listing on screen, once a fresh
    /// listing agrees. Never answers now.
    fn save_at(&mut self, folder: &Path, name: OsString) -> bool {
        if self
            .pick
            .as_ref()
            .is_some_and(|pick| pick.mode == Mode::Save)
        {
            self.save_new(folder, name);
        }
        false
    }

    /// The entry `found` of the folder on screen, named or clicked: goes
    /// into a folder (emptying the box), picks a file, or asks before a
    /// save over it. Whether it answered.
    fn accept_found(
        &mut self,
        folder: &Path,
        (kind, _, name): (EntryKind, EntryId, OsString),
    ) -> bool {
        let Some(pick) = &self.pick else { return false };
        match (pick.mode, kind) {
            (_, EntryKind::Folder) => self.jump(&folder.join(name), false),
            (Mode::Files, EntryKind::File) => self.answer(folder.join(name)),
            (Mode::Save, EntryKind::File) => {
                self.ask_over(folder, name);
                false
            }
            (Mode::Folders, EntryKind::File) => false,
        }
    }

    /// Goes to the typed `path`. With `fallback`, a path the core can't
    /// list sends the window to the folder it is in, with its last part
    /// in the name box. Never answers.
    fn jump(&mut self, path: &Path, fallback: bool) -> bool {
        let path = tidy(path);
        self.open(&path);
        let listing = self.asked;
        if let Some(pick) = &mut self.pick {
            pick.jump = Some(Jump {
                fallback,
                name: OsString::new(),
                listing,
            });
        }
        false
    }

    /// A folder is now on screen: a new listing drops the
    /// selection and any question, and a typed jump that led here leaves
    /// its name in the box.
    pub fn pick_folder_opened(&mut self, cx: &mut Context<Self>) {
        let listed = self.listing_id();
        if self.pick.is_none() {
            return;
        }
        self.selection = None;
        let Some(pick) = &mut self.pick else { return };
        pick.conflict = None;
        pick.copied = None;
        if let Some(jump) = pick.jump.take()
            && jump.listing == listed
        {
            pick.name.update(cx, |field, cx| {
                field.set_text(&jump.name.to_string_lossy(), cx);
            });
            pick.placed = Some(jump.name).filter(|name| !name.is_empty());
        }
        // Only the listing a save asked for answers it; any other one came
        // from going somewhere else, which drops the save.
        if let Some(recheck) = pick.recheck.take()
            && recheck.listing == listed
        {
            self.save_checked(recheck, cx);
        }
    }

    /// The core couldn't list `path`. A typed path that may name a file
    /// goes to the folder it is in instead.
    pub fn pick_folder_refused(&mut self, id: ListingId, path: &Path) {
        let Some(pick) = &mut self.pick else { return };
        // A save whose folder is gone waits for the user again, and one
        // that going somewhere else replaced is dropped.
        pick.recheck = None;
        let Some(jump) = pick.jump.take().filter(|jump| jump.listing == Some(id)) else {
            return;
        };
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return;
        };
        if !jump.fallback {
            return;
        }
        let name = name.to_owned();
        self.open(parent);
        let listing = self.asked;
        if let Some(pick) = &mut self.pick {
            pick.jump = Some(Jump {
                fallback: false,
                name,
                listing,
            });
        }
    }
}
