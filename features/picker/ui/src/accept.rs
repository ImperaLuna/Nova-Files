use std::ffi::OsString;
use std::path::{Path, PathBuf};

use gpui::Context;
use nova_files_api::{EntryId, EntryKind, Listing};
use nova_files_shell::typed::{self, Typed};

use crate::Mode;
use crate::part::{Jump, PickerPart, entry_at};

type Found = (EntryKind, EntryId, OsString);

impl PickerPart {
    pub fn accept(&mut self, cx: &mut Context<Self>) {
        if self.waiting(cx) {
            return;
        }
        let Some(pick) = &self.pick else { return };
        if pick.conflict.is_some() {
            self.keep_both(cx);
            cx.notify();
            return;
        }
        let folder = self.folder(cx);
        let text = pick.name.read(cx).text().to_owned();
        let clicked = pick
            .copied
            .as_ref()
            .filter(|(_, copied)| *copied == text)
            .map(|(ids, _)| self.shown_entries(ids, cx))
            .filter(|found| !found.is_empty());
        if let Some(mut found) = clicked {
            if found.len() == 1 {
                if let Some(found) = found.pop() {
                    self.accept_found(&folder, found, cx);
                }
            } else {
                let paths = found.into_iter().map(|(_, _, name)| folder.join(name));
                self.answer_all(paths.collect(), cx);
            }
            cx.notify();
            return;
        }
        let exact = pick
            .placed
            .clone()
            .filter(|placed| placed.to_string_lossy() == text);
        if let Some(suggested) = exact {
            self.accept_exact(&folder, suggested, cx);
            cx.notify();
            return;
        }
        match typed::read(&text, &self.host.home, &folder) {
            Typed::Nothing => self.accept_selection(&folder, cx),
            Typed::Folder(path) => self.jump(&path, false, cx),
            Typed::Path(path) => self.jump(&path, true, cx),
            Typed::Name(name) => self.accept_name(&folder, &name, cx),
        }
        cx.notify();
    }

    fn shown_entries(&self, ids: &[EntryId], cx: &Context<Self>) -> Vec<Found> {
        let entries = self.host.entries.read(cx);
        let shown = entries
            .listing()
            .map(|listing| &*listing.entries)
            .unwrap_or_default();
        ids.iter()
            .filter_map(|&id| entry_at(shown, id))
            .map(|entry| (entry.kind, entry.id, entry.name.clone()))
            .collect()
    }

    fn accept_selection(&mut self, folder: &Path, cx: &mut Context<Self>) {
        let Some(pick) = &self.pick else { return };
        let mode = pick.mode;
        let wanted = match mode {
            Mode::Files => EntryKind::File,
            Mode::Folders => EntryKind::Folder,
            Mode::Save => return,
        };
        let selected: Vec<Found> = self
            .host
            .entries
            .read(cx)
            .selected()
            .iter()
            .map(|entry| (entry.kind, entry.id, entry.name.clone()))
            .collect();
        let paths: Vec<PathBuf> = selected
            .iter()
            .filter(|&&(kind, _, _)| kind == wanted)
            .map(|(_, _, name)| folder.join(name))
            .collect();
        match (mode, paths.is_empty()) {
            (_, false) => {
                self.answer_all(paths, cx);
            }
            (Mode::Folders, true) => {
                self.answer(folder.to_path_buf(), cx);
            }
            (Mode::Files | Mode::Save, true) => {
                if let [(EntryKind::Folder, _, name)] = &selected[..] {
                    self.go(folder.join(name), cx);
                }
            }
        }
    }

    fn named(&self, cx: &Context<Self>, matches: impl Fn(&OsString) -> bool) -> Option<Found> {
        let entries = self.host.entries.read(cx);
        let entry = entries
            .listing()?
            .entries
            .iter()
            .find(|entry| matches(&entry.name))?;
        Some((entry.kind, entry.id, entry.name.clone()))
    }

    fn accept_name(&mut self, folder: &Path, name: &str, cx: &mut Context<Self>) {
        let found = self
            .named(cx, |entry| entry == name)
            .or_else(|| self.named(cx, |entry| entry.to_string_lossy() == name));
        match found {
            Some(found) => self.accept_found(folder, found, cx),
            None => self.save_at(folder, OsString::from(name), cx),
        }
    }

    fn accept_exact(&mut self, folder: &Path, name: OsString, cx: &mut Context<Self>) {
        match self.named(cx, |entry| *entry == name) {
            Some(found) => self.accept_found(folder, found, cx),
            None => self.save_at(folder, name, cx),
        }
    }

    fn save_at(&mut self, folder: &Path, name: OsString, cx: &mut Context<Self>) {
        if self
            .pick
            .as_ref()
            .is_some_and(|pick| pick.mode == Mode::Save)
        {
            self.save_new(folder, name, cx);
        }
    }

    fn accept_found(&mut self, folder: &Path, (kind, _, name): Found, cx: &mut Context<Self>) {
        let Some(pick) = &self.pick else { return };
        match (pick.mode, kind) {
            (_, EntryKind::Folder) => self.jump(&folder.join(name), false, cx),
            (Mode::Files, EntryKind::File) => {
                self.answer(folder.join(name), cx);
            }
            (Mode::Save, EntryKind::File) => self.ask_over(folder, name, cx),
            (Mode::Folders, EntryKind::File) => {}
        }
    }

    fn jump(&mut self, path: &Path, fallback: bool, cx: &mut Context<Self>) {
        let folder = tidy(path);
        self.go(folder.clone(), cx);
        if let Some(pick) = &mut self.pick {
            pick.jump = Some(Jump {
                fallback,
                name: OsString::new(),
                folder,
            });
        }
    }

    pub(crate) fn folder_opened(
        &mut self,
        listing: &Listing,
        reordered: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(pick) = &mut self.pick else { return };
        if reordered {
            let text = pick.name.read(cx).text();
            let unchanged = pick
                .copied
                .as_ref()
                .is_some_and(|(_, copied)| copied == text);
            if unchanged {
                self.selection_changed(cx);
            } else {
                pick.copied = None;
            }
            return;
        }
        pick.conflict = None;
        pick.copied = None;
        if let Some(jump) = pick.jump.take().filter(|jump| jump.folder == listing.path) {
            pick.name.update(cx, |field, cx| {
                field.set_text(&jump.name.to_string_lossy(), cx);
            });
            pick.placed = Some(jump.name).filter(|name| !name.is_empty());
        }
        if let Some(recheck) = pick
            .recheck
            .take()
            .filter(|recheck| recheck.probe.is_none() && recheck.folder == listing.path)
        {
            self.save_checked(recheck, cx);
        }
    }

    pub(crate) fn folder_refused(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(pick) = &mut self.pick else { return };
        pick.recheck = None;
        let Some(jump) = pick
            .jump
            .take()
            .filter(|jump| jump.folder == path && jump.fallback)
        else {
            return;
        };
        let (Some(parent), Some(name)) = (jump.folder.parent(), jump.folder.file_name()) else {
            return;
        };
        let (parent, name) = (parent.to_path_buf(), name.to_owned());
        self.go(parent.clone(), cx);
        if let Some(pick) = &mut self.pick {
            pick.jump = Some(Jump {
                fallback: false,
                name,
                folder: parent,
            });
        }
    }
}

fn tidy(path: &Path) -> PathBuf {
    let mut tidy = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => match tidy.components().next_back() {
                Some(std::path::Component::Normal(_)) => {
                    tidy.pop();
                }
                Some(std::path::Component::RootDir | std::path::Component::Prefix(_)) => {}
                Some(std::path::Component::ParentDir | std::path::Component::CurDir) | None => {
                    tidy.push(part);
                }
            },
            std::path::Component::RootDir
            | std::path::Component::Prefix(_)
            | std::path::Component::Normal(_) => tidy.push(part),
        }
    }
    tidy
}
