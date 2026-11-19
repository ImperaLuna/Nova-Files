use std::ffi::OsString;
use std::path::{Path, PathBuf};

use gpui::{Context, Window};
use nova_files_api::{EntryId, EntryKind, ListingId};

use super::Mode;
use crate::window::{MainWindow, tidy};
use nova_files_shell::typed;

pub(super) struct Jump {
    fallback: bool,
    name: OsString,
    listing: Option<ListingId>,
}

impl MainWindow {
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
                let paths = found.into_iter().map(|(_, _, name)| folder.join(name));
                self.answer_all(paths.collect())
            };
            if answered {
                window.remove_window();
            }
            cx.notify();
            return;
        }
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
            (Mode::Files | Mode::Save, true) => {
                if let [(EntryKind::Folder, id, _)] = selected[..] {
                    self.open_entry(id);
                }
                false
            }
        }
    }

    fn accept_name(&mut self, folder: &Path, name: &str) -> bool {
        let found = self.listing.as_ref().and_then(|listing| {
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

    pub fn pick_folder_opened(&mut self, reordered: bool, cx: &mut Context<Self>) {
        let listed = self.listing_id();
        let Some(pick) = &self.pick else { return };
        if reordered {
            let text = pick.name.read(cx).text();
            let unchanged = pick
                .copied
                .as_ref()
                .is_some_and(|(_, copied)| copied == text);
            if unchanged {
                self.pick_selection_changed(cx);
            } else if let Some(pick) = &mut self.pick {
                pick.copied = None;
            }
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
        if let Some(recheck) = pick.recheck.take()
            && recheck.listing == listed
        {
            self.save_checked(recheck);
        }
    }

    pub fn pick_folder_refused(&mut self, id: ListingId, path: &Path) {
        let Some(pick) = &mut self.pick else { return };
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
