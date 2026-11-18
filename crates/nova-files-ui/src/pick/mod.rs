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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Files,
    Folders,
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

    const fn label(self) -> &'static str {
        match self {
            Self::Files | Self::Folders => "Open",
            Self::Save => "Save",
        }
    }
}

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

pub fn accept_label(request: &PickRequest) -> String {
    request.accept_label.as_deref().map_or_else(
        || Mode::of(&request.kind).label().to_owned(),
        without_mnemonic,
    )
}

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

pub struct Picker {
    request: Box<PickRequest>,
    mode: Mode,
    pub name: Entity<TextField>,
    pub conflict: Option<Conflict>,
    jump: Option<Jump>,
    copied: Option<(Vec<EntryId>, String)>,
    placed: Option<OsString>,
    recheck: Option<save::Recheck>,
    window: AnyWindowHandle,
    filter: Option<usize>,
    filter_menu: bool,
    size: Size<Pixels>,
    _name_events: Subscription,
    _bounds: Subscription,
}

pub struct Conflict {
    pub path: PathBuf,
    pub keep_both: OsString,
}

impl Picker {
    pub const fn wanted(&self) -> EntryKind {
        match self.mode {
            Mode::Folders => EntryKind::Folder,
            Mode::Files | Mode::Save => EntryKind::File,
        }
    }

    pub(crate) const fn several(&self) -> bool {
        matches!(self.request.kind, PickKind::Open { multiple: true, .. })
    }

    fn exact_name(&self, text: &str) -> Option<OsString> {
        self.placed
            .clone()
            .filter(|placed| placed.to_string_lossy() == text)
    }
}

fn is_plain_name(name: &OsStr) -> bool {
    let mut parts = Path::new(name).components();
    matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(part)), None) if part == name
    )
}

impl MainWindow {
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

    pub fn pick_id(&self) -> Option<PickId> {
        self.pick.as_ref().map(|pick| pick.request.id)
    }

    pub fn listing_id(&self) -> Option<ListingId> {
        self.listing.as_ref().map(|listing| listing.id)
    }

    pub fn pick_name(&self, cx: &App) -> Option<String> {
        let pick = self.pick.as_ref()?;
        Some(pick.name.read(cx).text().to_owned())
    }

    pub fn pick_conflict(&self) -> Option<&OsStr> {
        let conflict = self.pick.as_ref()?.conflict.as_ref()?;
        Some(&conflict.keep_both)
    }

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

    fn entry(&self, listing: ListingId, id: EntryId) -> Option<&Entry> {
        let shown = self.listing.as_ref().filter(|shown| shown.id == listing)?;
        usize::try_from(id.0)
            .ok()
            .and_then(|index| shown.entries.get(index))
    }

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

    pub fn cancel(&mut self, window: &mut Window) {
        drop(self.take_pick());
        window.remove_window();
    }

    pub fn answer(&mut self, path: PathBuf) -> bool {
        self.answer_all(vec![path])
    }

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
