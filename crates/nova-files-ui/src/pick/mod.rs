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
//!
//! When the app asks for several entries, Ctrl+click and Shift+click
//! select more (see `select`). The box then shows each file's name in
//! quotes, and Open, while the box is unchanged, picks them all.

mod bar;
mod filter;
mod save;
mod select;
mod typed;

use std::ffi::{OsStr, OsString};
use std::io::{self, Write as _};
use std::path::{Component, Path, PathBuf};

use gpui::{
    AnyWindowHandle, App, AppContext as _, ClickEvent, Context, Entity, Focusable as _,
    Subscription, TitlebarOptions, Window,
};
use nova_files_api::{
    Entry, EntryId, EntryKind, ListingId, PickAnswer, PickId, PickKind, PickRequest,
};

use self::select::{Click, Selection};
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
    /// The entries clicked, in the listing they belong to.
    selected: Option<Selection>,
    /// A save onto a file that is there, waiting for Replace or Keep both.
    pub conflict: Option<Conflict>,
    /// A typed path the window is going to.
    jump: Option<Jump>,
    /// The entries clicks put in the name box and the text they put
    /// there: while the box still holds that text, it means those entries,
    /// whatever their names' bytes or spaces.
    copied: Option<(Vec<EntryId>, String)>,
    /// A name the window put in the box, the app's suggestion or a typed
    /// path's last part: until the user edits the box, it is that exact
    /// name, never typed text to read again.
    placed: Option<OsString>,
    /// A save waiting for a fresh listing of its folder (see `save`).
    recheck: Option<save::Recheck>,
    /// The picker's window, to close once a save is answered from a
    /// listing.
    window: AnyWindowHandle,
    /// The type filter chosen, as an index into the app's filters; `None`
    /// for All files.
    filter: Option<usize>,
    /// Whether the filter menu is open.
    filter_menu: bool,
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
    /// Whether a refusal goes to the folder the path is in instead.
    fallback: bool,
    /// What the name box holds once the window is there.
    name: OsString,
    /// The listing it asked for; any other one means the user went
    /// somewhere else, which drops it.
    listing: Option<ListingId>,
}

impl Picker {
    /// Whether the app takes more than one entry.
    const fn several(&self) -> bool {
        matches!(self.request.kind, PickKind::Open { multiple: true, .. })
    }

    /// The exact name the window put in the box, while the box still
    /// shows it unedited.
    fn exact_name(&self, text: &str) -> Option<OsString> {
        self.placed
            .clone()
            .filter(|placed| placed.to_string_lossy() == text)
    }
}

/// Whether `name` is one file name, not a path or a folder's dots.
fn is_plain_name(name: &OsStr) -> bool {
    let mut parts = Path::new(name).components();
    matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(part)), None) if part == name
    )
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
        let first_filter = filter::first(&request);
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
                    // A new name takes back the question, any save still
                    // waiting on the old one, and the name a typed path
                    // would leave in the box.
                    if let Some(pick) = &mut this.pick {
                        pick.conflict = None;
                        pick.recheck = None;
                        pick.jump = None;
                        pick.placed = None;
                        pick.copied = None;
                    }
                    cx.notify();
                }
            });
        // Only a plain suggestion is taken as it came: one with a `/`, or
        // `.` or `..`, is read as typed, so a save never lands outside the
        // folder on screen unasked.
        let placed = match &request.kind {
            PickKind::Save { name } if is_plain_name(name) => Some(name.clone()),
            _ => None,
        };
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
            placed,
            recheck: None,
            window: window.window_handle(),
            filter: first_filter,
            filter_menu: false,
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

    /// The entries a picker has selected, in the listing they belong to,
    /// while there are any.
    pub fn pick_selection(&self) -> Option<(ListingId, Vec<EntryId>)> {
        let selection = self.pick.as_ref()?.selected.as_ref()?;
        let ids = selection.ids();
        (!ids.is_empty()).then(|| (selection.listing, ids.to_vec()))
    }

    /// Whether the row `entry` shows as one of the picker's selection.
    pub fn pick_selects(&self, (listing, id): (ListingId, EntryId)) -> bool {
        self.pick
            .as_ref()
            .and_then(|pick| pick.selected.as_ref())
            .is_some_and(|selection| selection.listing == listing && selection.contains(id))
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
        let Some(pick) = &mut self.pick else {
            if kind == EntryKind::Folder {
                self.open_entry(id);
            }
            return;
        };
        let click = Click::of(event.modifiers(), pick.several());
        match (kind, pick.mode, click) {
            (EntryKind::Folder, Mode::Folders, Click::Only) if double => self.open_entry(id),
            (EntryKind::Folder, Mode::Folders, _) => self.select(listing, id, click, cx),
            (EntryKind::Folder, _, Click::Only) => self.open_entry(id),
            // Ctrl or Shift on a folder in a picker for files would lose
            // the files selected, so it does nothing.
            (EntryKind::Folder, _, _) | (EntryKind::File, Mode::Folders, _) => {}
            (EntryKind::File, Mode::Files, Click::Only) if double => {
                if self.pick_entry(listing, id) {
                    window.remove_window();
                }
            }
            (EntryKind::File, Mode::Files | Mode::Save, _) => self.select(listing, id, click, cx),
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
        let paths: Vec<PathBuf> = pick
            .selected
            .as_ref()
            .filter(|selection| self.listing_id() == Some(selection.listing))
            .map(|selection| self.shown_entries(selection.ids()))
            .unwrap_or_default()
            .into_iter()
            .filter(|&(kind, _, _)| kind == wanted)
            .map(|(_, _, name)| folder.join(name))
            .collect();
        match (mode, paths.is_empty()) {
            (_, false) => self.answer_all(paths),
            (Mode::Folders, true) => self.answer(folder.to_path_buf()),
            (Mode::Files | Mode::Save, true) => false,
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
        let Some(pick) = &mut self.pick else { return };
        pick.selected = None;
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

    /// Cancel, or Escape in the name box: answers cancelled and closes.
    pub fn cancel(&mut self, window: &mut Window) {
        // Answered now, not when the window's view is freed.
        self.pick = None;
        window.remove_window();
    }

    /// Answers the app with `path`. Whether it answered, so the window can
    /// close.
    pub fn answer(&mut self, path: PathBuf) -> bool {
        self.answer_all(vec![path])
    }

    /// Answers the app with `paths`. Whether it answered.
    fn answer_all(&mut self, paths: Vec<PathBuf>) -> bool {
        let Some(pick) = self.pick.take() else {
            return false;
        };
        let filter = pick.filter;
        pick.request
            .reply
            .answer(PickAnswer::Picked { paths, filter });
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
            types: None,
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
    fn only_a_plain_name_is_taken_as_it_came() {
        for plain in ["notes.txt", " notes.txt", "~", "..notes"] {
            assert!(is_plain_name(OsStr::new(plain)), "{plain:?}");
        }
        for path in [
            "",
            ".",
            "..",
            "../notes.txt",
            "/etc/passwd",
            "a/b",
            "notes.txt/",
        ] {
            assert!(!is_plain_name(OsStr::new(path)), "{path:?}");
        }
    }

    #[test]
    fn a_picker_starts_on_the_type_the_app_asks_for_or_its_first() {
        let with = |count: usize, chosen| PickRequest {
            filters: (0..count)
                .map(|at| nova_files_api::FileFilter {
                    name: at.to_string(),
                    rules: Vec::new(),
                })
                .collect(),
            filter: chosen,
            ..request("", FILES)
        };
        assert_eq!(filter::first(&with(3, Some(2))), Some(2));
        assert_eq!(filter::first(&with(3, None)), Some(0));
        assert_eq!(filter::first(&with(3, Some(3))), Some(0));
        assert_eq!(filter::first(&with(0, None)), None);
        assert_eq!(filter::first(&with(0, Some(0))), None);
    }

    #[test]
    fn the_menu_names_each_type_and_marks_the_one_chosen() {
        let request = PickRequest {
            filters: vec![nova_files_api::FileFilter {
                name: "Text".to_owned(),
                rules: Vec::new(),
            }],
            ..request("", FILES)
        };
        assert_eq!(filter::label(&request, Some(0)), "Text");
        assert_eq!(filter::label(&request, None), "All files");
        assert_eq!(filter::label(&request, Some(1)), "All files");
        assert!(filter::is_chosen(Some(0), Some(0)));
        assert!(filter::is_chosen(None, None));
        assert!(!filter::is_chosen(Some(0), None));
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
