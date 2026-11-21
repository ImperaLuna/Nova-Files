use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::Path;

use gpui::Context;
use nova_files_api::numbered::{Naming, first_numbered};
use nova_files_properties_api::{Command, PropertiesId};

use crate::part::{Conflict, PickerPart, Recheck};

impl PickerPart {
    pub(crate) fn save_new(&mut self, folder: &Path, name: OsString, cx: &mut Context<Self>) {
        self.recheck(folder, name, None, cx);
    }

    fn recheck(
        &mut self,
        folder: &Path,
        name: OsString,
        beside: Option<OsString>,
        cx: &mut Context<Self>,
    ) {
        self.go(folder.to_path_buf(), cx);
        if let Some(pick) = &mut self.pick {
            pick.recheck = Some(Recheck {
                folder: folder.to_path_buf(),
                name,
                beside,
                probe: None,
            });
        }
    }

    pub(crate) fn ask_over(&mut self, folder: &Path, name: OsString, cx: &mut Context<Self>) {
        let (keep_both, there) = {
            let entries = self.host.entries.read(cx);
            let shown = entries
                .listing()
                .map(|listing| &*listing.entries)
                .unwrap_or_default();
            let names: HashSet<OsString> = shown.iter().map(|entry| folded(&entry.name)).collect();
            let keep_both = first_numbered(&name, Naming::File, |candidate| {
                names.contains(&folded(candidate))
            })
            .unwrap_or_else(|| name.clone());
            let there = shown
                .iter()
                .find(|entry| entry.name == name)
                .or_else(|| {
                    let name = folded(&name);
                    shown.iter().find(|entry| folded(&entry.name) == name)
                })
                .map(|entry| entry.name.clone());
            (keep_both, there)
        };
        if let Some(pick) = &mut self.pick {
            pick.quiet = true;
        }
        self.host.entries.update(cx, |entries, cx| {
            entries.select(there.into_iter().collect(), cx);
        });
        if let Some(pick) = &mut self.pick {
            pick.conflict = Some(Conflict {
                path: folder.join(name),
                keep_both,
            });
        }
    }

    pub(crate) fn save_checked(&mut self, recheck: Recheck, cx: &mut Context<Self>) {
        let name = folded(&recheck.name);
        let taken = self.host.entries.read(cx).listing().is_some_and(|listing| {
            listing
                .entries
                .iter()
                .any(|entry| folded(&entry.name) == name)
        });
        if taken {
            let over = recheck.beside.unwrap_or(recheck.name);
            self.ask_over(&recheck.folder, over, cx);
            return;
        }
        self.probe(recheck, cx);
    }

    fn probe(&mut self, mut recheck: Recheck, cx: &Context<Self>) {
        let Some(id) = self
            .host
            .entries
            .read(cx)
            .listing()
            .map(|listing| PropertiesId(listing.id.0))
        else {
            return;
        };
        let path = recheck.folder.join(&recheck.name);
        let sent = self
            .host
            .link
            .send(Command::Properties { id, path })
            .is_ok();
        if let Some(pick) = self.pick.as_mut().filter(|_| sent) {
            recheck.probe = Some(id);
            pick.recheck = Some(recheck);
        }
    }

    pub(crate) fn probed(&mut self, id: PropertiesId, missing: bool, cx: &mut Context<Self>) {
        let Some(recheck) = self
            .pick
            .as_mut()
            .and_then(|pick| pick.recheck.take_if(|recheck| recheck.probe == Some(id)))
        else {
            return;
        };
        if missing {
            self.answer(recheck.folder.join(recheck.name), cx);
        } else {
            let over = recheck.beside.unwrap_or(recheck.name);
            self.ask_over(&recheck.folder, over, cx);
        }
        cx.notify();
    }

    pub fn replace(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self
            .pick
            .as_mut()
            .and_then(|pick| pick.conflict.take())
            .map(|conflict| conflict.path)
        else {
            return;
        };
        self.answer(path, cx);
    }

    pub fn keep_both(&mut self, cx: &mut Context<Self>) {
        let Some(conflict) = self.pick.as_mut().and_then(|pick| pick.conflict.take()) else {
            return;
        };
        let (Some(folder), Some(beside)) = (conflict.path.parent(), conflict.path.file_name())
        else {
            return;
        };
        let folder = folder.to_path_buf();
        self.recheck(&folder, conflict.keep_both, Some(beside.to_owned()), cx);
    }
}

fn folded(name: &OsStr) -> OsString {
    name.to_str()
        .map_or_else(|| name.to_owned(), |text| text.to_lowercase().into())
}
