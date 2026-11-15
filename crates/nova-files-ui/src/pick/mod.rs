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
//! The arrow keys, which the name box passes on, move through the
//! entries as in a main window, and Enter with nothing typed goes into a
//! folder they landed on. When the app asks for several entries,
//! Ctrl+click and Shift+click select more (see `select`). The box then
//! shows each file's name in quotes, and Open, while the box is
//! unchanged, picks them all.

mod accept;
mod bar;
mod filter;
mod opening;
mod save;
mod select;
pub mod typed;

pub use typed::{HOME, stem};

use std::ffi::{OsStr, OsString};
use std::io::{self, Write as _};
use std::path::{Component, Path, PathBuf};

use gpui::{
    AnyWindowHandle, App, AppContext as _, ClickEvent, Context, Entity, Focusable as _, Pixels,
    Size, Subscription, TitlebarOptions, Window,
};
use nova_files_api::{
    Entry, EntryId, EntryKind, ListingId, PickAnswer, PickId, PickKind, PickRequest,
};

use self::accept::Jump;
pub use self::opening::foreign_parent;
use crate::selecting::DOUBLE_CLICK;
use crate::selection::Click;
use crate::text::{TextField, TextFieldEvent};
use crate::window::MainWindow;

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
    /// The window's size, kept as it changes.
    size: Size<Pixels>,
    _name_events: Subscription,
    _bounds: Subscription,
}

/// A save onto a file the folder on screen has.
pub struct Conflict {
    /// The file that is there.
    pub path: PathBuf,
    /// The free name that keeps both.
    pub keep_both: OsString,
}

impl Picker {
    /// What the picker picks: a range holds only these.
    pub const fn wanted(&self) -> EntryKind {
        match self.mode {
            Mode::Folders => EntryKind::Folder,
            Mode::Files | Mode::Save => EntryKind::File,
        }
    }

    /// Whether the app takes more than one entry.
    pub(crate) const fn several(&self) -> bool {
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
                field.select_range(stem(&text), cx);
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
        let (size, bounds) = opening::watch_size(window, cx);
        self.pick = Some(Picker {
            request,
            mode,
            name,
            conflict: None,
            jump: None,
            copied: None,
            placed,
            recheck: None,
            window: window.window_handle(),
            filter: first_filter,
            filter_menu: false,
            size,
            _name_events: name_events,
            _bounds: bounds,
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
    /// selects and a double-click opens (see `selecting`).
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
            self.main_clicked((listing, id), event, cx);
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

    /// Cancel, or Escape in the name box: answers cancelled and closes.
    pub fn cancel(&mut self, window: &mut Window) {
        // Answered now, not when the window's view is freed.
        drop(self.take_pick());
        window.remove_window();
    }

    /// Answers the app with `path`. Whether it answered, so the window can
    /// close.
    pub fn answer(&mut self, path: PathBuf) -> bool {
        self.answer_all(vec![path])
    }

    /// Answers the app with `paths`. Whether it answered.
    fn answer_all(&mut self, paths: Vec<PathBuf>) -> bool {
        let Some(pick) = self.take_pick() else {
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
            parent: None,
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
