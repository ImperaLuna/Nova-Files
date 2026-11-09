//! Pick mode: the main window opened for another app's Open or Save
//! dialog, with the pick bar along its bottom (see `bar`).
//!
//! The window holds the app's request and answers it once, then closes.
//! Closing it any other way drops the request, which answers cancelled;
//! the portal may close the request first, and [`close`] then closes its
//! window.
//!
//! A click selects an entry and, for files, copies its name into the name
//! box. A double-click on a file picks it in an Open dialog for files; in
//! one for folders a double-click goes into the folder, since a click
//! selects it. Enter in the name box, or the Open or Save button, reads
//! what is typed (see `typed`): `~/`, `/etc` and other paths go there, a
//! name in the folder on screen is opened, picked or saved at, and nothing
//! typed picks the selection (or, for folders, the folder on screen). A
//! save onto a file that is there asks in the bar: Replace, or Keep both
//! under a numbered name, which Enter chooses.
//!
//! The window never touches the disk, so what is there is what the folder
//! on screen lists, and a typed path is tried as a folder first: if the
//! core can't list it, the window goes to the folder it is in instead,
//! with its last part as the name. A save answers only from a fresh
//! listing of its folder (see `save`). A name a click put in the box means
//! the entry clicked, until the box is edited.

mod bar;
mod save;
mod typed;

use std::ffi::{OsStr, OsString};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use gpui::{
    AnyWindowHandle, App, AppContext as _, ClickEvent, Context, Entity, Focusable as _,
    Subscription, TitlebarOptions, Window,
};
use nova_files_api::{
    Entry, EntryId, EntryKind, ListingId, PickAnswer, PickId, PickKind, PickRequest,
};

use crate::text::{TextField, TextFieldEvent};
use crate::window::{MainWindow, tidy};

/// The clicks in a double-click.
const DOUBLE_CLICK: usize = 2;

/// What a picker is for, from what the app asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Files to open.
    Files,
    /// A folder to open.
    Folders,
    /// A place to save one file.
    Save,
}

impl Mode {
    pub const fn of(kind: &PickKind) -> Self {
        match kind {
            PickKind::Open { folders: false, .. } => Self::Files,
            PickKind::Open { folders: true, .. } => Self::Folders,
            PickKind::Save { .. } => Self::Save,
        }
    }

    /// The window title and the button's label when the app gives none.
    const fn label(self) -> &'static str {
        match self {
            Self::Files | Self::Folders => "Open",
            Self::Save => "Save",
        }
    }
}

/// The title bar of a picker for `request`, titled as the app asks or by
/// what it is for; the portal lets an app leave the title out.
pub fn titlebar(request: &PickRequest) -> TitlebarOptions {
    let title = if request.title.is_empty() {
        Mode::of(&request.kind).label()
    } else {
        &request.title
    };
    TitlebarOptions {
        title: Some(title.to_owned().into()),
        ..TitlebarOptions::default()
    }
}

/// The label on the Open or Save button: the app's, or what it is for.
pub fn accept_label(request: &PickRequest) -> String {
    request.accept_label.as_deref().map_or_else(
        || Mode::of(&request.kind).label().to_owned(),
        without_mnemonic,
    )
}

/// GTK's mnemonic marks: `_` before the letter to underline (`_OK`), and
/// `__` for an underscore itself. The bar has no mnemonics, so a single
/// `_` goes.
fn without_mnemonic(label: &str) -> String {
    let mut plain = String::with_capacity(label.len());
    let mut chars = label.chars();
    while let Some(c) = chars.next() {
        match (c, chars.clone().next()) {
            (MNEMONIC, Some(MNEMONIC)) => {
                plain.push(MNEMONIC);
                chars.next();
            }
            (MNEMONIC, _) => {}
            (c, _) => plain.push(c),
        }
    }
    plain
}

const MNEMONIC: char = '_';

/// A window's pick: the app's request and the bar's state.
pub struct Picker {
    request: Box<PickRequest>,
    mode: Mode,
    /// The name box.
    pub name: Entity<TextField>,
    /// The entry clicked last, in the listing it belongs to.
    selected: Option<(ListingId, EntryId)>,
    /// A save onto a file that is there, waiting for Replace or Keep both.
    pub conflict: Option<Conflict>,
    /// A typed path the window is going to.
    jump: Option<Jump>,
    /// The entry a click put in the name box and the text it put there:
    /// while the box still holds that text, it means that entry, whatever
    /// its name's bytes or spaces.
    copied: Option<(EntryId, String)>,
    /// A save waiting for a fresh listing of its folder (see `save`).
    recheck: Option<save::Recheck>,
    /// The picker's window, to close once a save is answered from a
    /// listing.
    window: AnyWindowHandle,
    _name_events: Subscription,
}

/// A save onto a file the folder on screen has.
pub struct Conflict {
    /// The file that is there.
    pub path: PathBuf,
    /// The free name that keeps both.
    pub keep_both: OsString,
}

/// A typed path the window asked the core for.
struct Jump {
    path: PathBuf,
    /// Whether a refusal goes to the folder the path is in instead.
    fallback: bool,
    /// What the name box holds once the window is there.
    name: String,
}

impl Picker {
    /// The app's suggested save name, if the box still shows it as it came.
    fn unchanged_suggestion(&self, text: &str) -> Option<OsString> {
        match &self.request.kind {
            PickKind::Save { name } if !name.is_empty() && name.to_string_lossy() == text => {
                Some(name.clone())
            }
            _ => None,
        }
    }
}

impl MainWindow {
    /// Puts the window in pick mode for `request`. A save starts with the
    /// app's name in the box, the part before its extension selected.
    pub fn set_pick(
        &mut self,
        request: Box<PickRequest>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mode = Mode::of(&request.kind);
        let name = cx.new(TextField::new);
        if let PickKind::Save { name: suggested } = &request.kind {
            let text = suggested.to_string_lossy();
            name.update(cx, |field, cx| {
                field.set_text(&text, cx);
                field.select_range(typed::stem(&text), cx);
            });
        }
        let name_events =
            cx.subscribe_in(&name, window, |this, _, event, window, cx| match event {
                TextFieldEvent::Submitted => this.accept(window, cx),
                // Escape takes back a question first, then cancels.
                TextFieldEvent::Cancelled => {
                    let asking = this
                        .pick
                        .as_mut()
                        .and_then(|pick| pick.conflict.take())
                        .is_some();
                    if !asking {
                        this.cancel(window);
                    }
                    cx.notify();
                }
                TextFieldEvent::Changed => {
                    // A new name takes back the question, and any save
                    // still waiting on the old one.
                    if let Some(pick) = &mut this.pick {
                        pick.conflict = None;
                        pick.recheck = None;
                    }
                    cx.notify();
                }
            });
        let focus = name.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        self.pick = Some(Picker {
            request,
            mode,
            name,
            selected: None,
            conflict: None,
            jump: None,
            copied: None,
            recheck: None,
            window: window.window_handle(),
            _name_events: name_events,
        });
    }

    /// The request the window answers, while it is a picker that hasn't.
    pub fn pick_id(&self) -> Option<PickId> {
        self.pick.as_ref().map(|pick| pick.request.id)
    }

    /// The listing on screen, which names the entries a pick refers to.
    pub fn listing_id(&self) -> Option<ListingId> {
        self.listing.as_ref().map(|listing| listing.id)
    }

    /// The entry a picker has selected, in the listing it belongs to.
    pub fn pick_selection(&self) -> Option<(ListingId, EntryId)> {
        self.pick.as_ref()?.selected
    }

    /// Whether the row `entry` shows as the picker's selection.
    pub fn pick_selects(&self, entry: (ListingId, EntryId)) -> bool {
        self.pick_selection() == Some(entry)
    }

    /// The text in the picker's name box.
    pub fn pick_name(&self, cx: &App) -> Option<String> {
        let pick = self.pick.as_ref()?;
        Some(pick.name.read(cx).text().to_owned())
    }

    /// The numbered name a save onto a file that is there would keep both
    /// under, while the bar asks.
    pub fn pick_conflict(&self) -> Option<&OsStr> {
        let conflict = self.pick.as_ref()?.conflict.as_ref()?;
        Some(&conflict.keep_both)
    }

    /// A click on the entry `id` of `listing`. Outside a picker a click
    /// goes into a folder and leaves a file alone.
    pub fn entry_clicked(
        &mut self,
        (listing, id): (ListingId, EntryId),
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let double = event.click_count() >= DOUBLE_CLICK;
        let Some(entry) = self.entry(listing, id) else {
            return;
        };
        let kind = entry.kind;
        let name = entry.name.to_string_lossy().into_owned();
        let Some(pick) = &mut self.pick else {
            if kind == EntryKind::Folder {
                self.open_entry(id);
            }
            return;
        };
        match (kind, pick.mode) {
            (EntryKind::Folder, Mode::Folders) if !double => pick.selected = Some((listing, id)),
            (EntryKind::Folder, _) => self.open_entry(id),
            (EntryKind::File, Mode::Files) if double => {
                if self.pick_entry(listing, id) {
                    window.remove_window();
                }
            }
            (EntryKind::File, Mode::Folders) => {}
            (EntryKind::File, Mode::Files | Mode::Save) => {
                pick.selected = Some((listing, id));
                pick.name.update(cx, |field, cx| field.set_text(&name, cx));
                pick.copied = Some((id, name));
                pick.conflict = None;
            }
        }
        cx.notify();
    }

    /// The entry `id` of `listing`, while that listing is on screen.
    fn entry(&self, listing: ListingId, id: EntryId) -> Option<&Entry> {
        let shown = self.listing.as_ref().filter(|shown| shown.id == listing)?;
        usize::try_from(id.0)
            .ok()
            .and_then(|index| shown.entries.get(index))
    }

    /// Picks the file `id` of `listing` and answers an Open dialog for
    /// files. Whether it answered, so the window can close. Nothing is
    /// picked once another listing replaced that one, or while a folder
    /// asked for is loading, since the user has already moved on.
    pub fn pick_entry(&mut self, listing: ListingId, id: EntryId) -> bool {
        if self.waiting() || self.pick.as_ref().map(|pick| pick.mode) != Some(Mode::Files) {
            return false;
        }
        let Some(path) = self
            .entry(listing, id)
            .filter(|entry| entry.kind == EntryKind::File)
            .and_then(|entry| Some(self.folder()?.join(&entry.name)))
        else {
            return false;
        };
        self.answer(path)
    }

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
            .and_then(|&(id, _)| {
                let entry = self.entry(self.listing_id()?, id)?;
                Some((entry.kind, id, entry.name.clone()))
            });
        if let Some(found) = clicked {
            if self.accept_found(&folder, found) {
                window.remove_window();
            }
            cx.notify();
            return;
        }
        // The app's own name, unchanged, is a name with its exact bytes,
        // never a path to read.
        if let Some(suggested) = pick.unchanged_suggestion(&text) {
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

    /// With nothing typed: picks the file or folder selected, or for
    /// folders the folder on screen. Whether it answered.
    fn accept_selection(&mut self, folder: &Path) -> bool {
        let Some(pick) = &self.pick else { return false };
        let mode = pick.mode;
        let selected = pick
            .selected
            .and_then(|(listing, id)| self.entry(listing, id))
            .map(|entry| (entry.kind, folder.join(&entry.name)));
        match (mode, selected) {
            (Mode::Files, Some((EntryKind::File, path)))
            | (Mode::Folders, Some((EntryKind::Folder, path))) => self.answer(path),
            (Mode::Folders, _) => self.answer(folder.to_path_buf()),
            (Mode::Files | Mode::Save, _) => false,
        }
    }

    /// `name` typed in the folder on screen: goes into it if it is a
    /// folder there; otherwise opens it as a file, or saves at it. Whether
    /// it answered.
    fn accept_name(&mut self, folder: &Path, name: &str) -> bool {
        let found = self.listing.as_ref().and_then(|listing| {
            let entry = listing
                .entries
                .iter()
                .find(|entry| entry.name.to_string_lossy() == name)?;
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
        if let Some(pick) = &mut self.pick {
            pick.jump = Some(Jump {
                path,
                fallback,
                name: String::new(),
            });
        }
        false
    }

    /// The folder `path` is now on screen: a new listing drops the
    /// selection and any question, and a typed jump that led here leaves
    /// its name in the box.
    pub fn pick_folder_opened(&mut self, path: &Path, cx: &mut Context<Self>) {
        let listed = self.listing_id();
        let Some(pick) = &mut self.pick else { return };
        pick.selected = None;
        pick.conflict = None;
        pick.copied = None;
        if let Some(jump) = pick.jump.take_if(|jump| jump.path == path) {
            pick.name
                .update(cx, |field, cx| field.set_text(&jump.name, cx));
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
    pub fn pick_folder_refused(&mut self, path: &Path) {
        let Some(pick) = &mut self.pick else { return };
        // A save whose folder is gone waits for the user again, and one
        // that going somewhere else replaced is dropped.
        pick.recheck = None;
        let Some(jump) = pick.jump.take_if(|jump| jump.path == path) else {
            return;
        };
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return;
        };
        if !jump.fallback {
            return;
        }
        let parent = parent.to_path_buf();
        let name = name.to_string_lossy().into_owned();
        self.open(&parent);
        if let Some(pick) = &mut self.pick {
            pick.jump = Some(Jump {
                path: parent,
                fallback: false,
                name,
            });
        }
    }

    /// Cancel, or Escape in the name box: answers cancelled and closes.
    pub fn cancel(&mut self, window: &mut Window) {
        // Answered now, not when the window's view is freed.
        self.pick = None;
        window.remove_window();
    }

    /// Answers the app with `path`. Whether it answered, so the window can
    /// close.
    pub fn answer(&mut self, path: PathBuf) -> bool {
        let Some(pick) = self.pick.take() else {
            return false;
        };
        let filter = pick.request.filter;
        pick.request.reply.answer(PickAnswer::Picked {
            paths: vec![path],
            filter,
        });
        true
    }
}

/// Closes the picker window for `id`, if one is open, and drops its
/// request, which answers cancelled.
pub fn close(id: PickId, cx: &mut App) {
    for handle in cx.windows() {
        let Some(handle) = handle.downcast::<MainWindow>() else {
            continue;
        };
        let closed = handle.update(cx, |view, window, _| {
            if view.pick_id() == Some(id) {
                view.cancel(window);
            }
        });
        if let Err(error) = closed {
            // A window that is going away anyway; its request is answered
            // when it is freed. Until there is a log, stderr is where this
            // goes.
            writeln!(
                io::stderr(),
                "nova-files: could not close a picker: {error:#}"
            )
            .ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use nova_files_api::PickReply;

    use super::*;

    fn request(title: &str, kind: PickKind) -> PickRequest {
        PickRequest {
            id: PickId(1),
            kind,
            title: title.to_owned(),
            accept_label: None,
            filters: Vec::new(),
            filter: None,
            folder: None,
            reply: PickReply::new(|_| {}),
        }
    }

    const FILES: PickKind = PickKind::Open {
        multiple: true,
        folders: false,
    };
    const FOLDERS: PickKind = PickKind::Open {
        multiple: false,
        folders: true,
    };

    #[test]
    fn a_picker_is_titled_as_its_app_asks_or_by_what_it_is_for() {
        let title = |request| titlebar(&request).title.map(|title| title.to_string());
        assert_eq!(
            title(request("Upload photos", FILES)),
            Some("Upload photos".to_owned())
        );
        assert_eq!(title(request("", FILES)), Some("Open".to_owned()));
        assert_eq!(title(request("", FOLDERS)), Some("Open".to_owned()));
        let save = PickKind::Save {
            name: "a.txt".into(),
        };
        assert_eq!(title(request("", save)), Some("Save".to_owned()));
    }

    #[test]
    fn the_button_says_what_the_app_asks_or_open_or_save() {
        let upload = PickRequest {
            accept_label: Some("Upload".to_owned()),
            ..request("", FILES)
        };
        assert_eq!(accept_label(&upload), "Upload");
        let mnemonic = PickRequest {
            accept_label: Some("_Save__as".to_owned()),
            ..request("", FILES)
        };
        assert_eq!(accept_label(&mnemonic), "Save_as");
        assert_eq!(accept_label(&request("", FOLDERS)), "Open");
        let save = PickKind::Save {
            name: "a.txt".into(),
        };
        assert_eq!(accept_label(&request("", save)), "Save");
    }

    #[test]
    fn the_mode_follows_what_the_app_asked() {
        assert_eq!(Mode::of(&FILES), Mode::Files);
        assert_eq!(Mode::of(&FOLDERS), Mode::Folders);
        let save = PickKind::Save {
            name: "a.txt".into(),
        };
        assert_eq!(Mode::of(&save), Mode::Save);
    }
}
