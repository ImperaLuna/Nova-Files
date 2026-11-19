use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use gpui::{Context, Window};

use nova_files_api::ListingId;
use nova_files_properties_api::{Command, PropertiesId};

use super::Conflict;
use crate::selection::Selection;
use crate::window::MainWindow;
use nova_files_shell::typed;

pub struct Recheck {
    folder: PathBuf,
    pub listing: Option<ListingId>,
    name: OsString,
    beside: Option<OsString>,
    pub probe: Option<PropertiesId>,
}

impl MainWindow {
    pub fn save_new(&mut self, folder: &Path, name: OsString) {
        self.recheck(folder, name, None);
    }

    fn recheck(&mut self, folder: &Path, name: OsString, beside: Option<OsString>) {
        self.open(folder);
        let listing = self.asked;
        if let Some(pick) = &mut self.pick {
            pick.recheck = Some(Recheck {
                folder: folder.to_path_buf(),
                listing,
                name,
                beside,
                probe: None,
            });
        }
    }

    pub fn ask_over(&mut self, folder: &Path, name: OsString) {
        let names: HashSet<OsString> = self
            .listing
            .iter()
            .flat_map(|listing| listing.entries.iter())
            .map(|entry| folded(&entry.name))
            .collect();
        let keep_both = typed::keep_both(&name, |candidate| names.contains(&folded(candidate)));
        let there = self.listing.as_ref().and_then(|listing| {
            let entry = listing
                .entries
                .iter()
                .find(|entry| entry.name == name)
                .or_else(|| {
                    let name = folded(&name);
                    listing
                        .entries
                        .iter()
                        .find(|entry| folded(&entry.name) == name)
                })?;
            Some((listing.id, entry.id))
        });
        self.selection = there.map(|(listing, id)| Selection::only(listing, id));
        if let Some(pick) = &mut self.pick {
            pick.conflict = Some(Conflict {
                path: folder.join(name),
                keep_both,
            });
        }
    }

    pub fn save_checked(&mut self, recheck: Recheck) {
        let name = folded(&recheck.name);
        let taken = self.listing.as_ref().is_some_and(|listing| {
            listing
                .entries
                .iter()
                .any(|entry| folded(&entry.name) == name)
        });
        if taken {
            let over = recheck.beside.unwrap_or(recheck.name);
            self.ask_over(&recheck.folder, over);
            return;
        }
        self.probe(recheck);
    }

    fn probe(&mut self, mut recheck: Recheck) {
        let Some(id) = recheck.listing.map(|listing| PropertiesId(listing.0)) else {
            return;
        };
        let path = recheck.folder.join(&recheck.name);
        let sent = self
            .connection
            .as_ref()
            .is_some_and(|connection| connection.send(Command::Properties { id, path }).is_ok());
        if let Some(pick) = self.pick.as_mut().filter(|_| sent) {
            recheck.probe = Some(id);
            pick.recheck = Some(recheck);
        }
    }

    pub fn save_probed(&mut self, id: PropertiesId, missing: bool, cx: &mut Context<Self>) {
        let Some(recheck) = self
            .pick
            .as_mut()
            .and_then(|pick| pick.recheck.take_if(|recheck| recheck.probe == Some(id)))
        else {
            return;
        };
        if missing {
            self.save_free(recheck, cx);
        } else {
            let over = recheck.beside.unwrap_or(recheck.name);
            self.ask_over(&recheck.folder, over);
            cx.notify();
        }
    }

    fn save_free(&mut self, recheck: Recheck, cx: &mut Context<Self>) {
        let Some(handle) = self.pick.as_ref().map(|pick| pick.window) else {
            return;
        };
        if self.answer(recheck.folder.join(recheck.name)) {
            cx.defer(move |cx| {
                handle
                    .update(cx, |_, window, _| window.remove_window())
                    .ok();
            });
        }
    }

    pub fn replace(&mut self, window: &mut Window) {
        let Some(path) = self
            .pick
            .as_mut()
            .and_then(|pick| pick.conflict.take())
            .map(|conflict| conflict.path)
        else {
            return;
        };
        if self.answer(path) {
            window.remove_window();
        }
    }

    pub fn keep_both(&mut self) {
        let Some(conflict) = self.pick.as_mut().and_then(|pick| pick.conflict.take()) else {
            return;
        };
        let (Some(folder), Some(beside)) = (conflict.path.parent(), conflict.path.file_name())
        else {
            return;
        };
        self.recheck(folder, conflict.keep_both, Some(beside.to_owned()));
    }
}

fn folded(name: &OsStr) -> OsString {
    name.to_str()
        .map_or_else(|| name.to_owned(), |text| text.to_lowercase().into())
}
