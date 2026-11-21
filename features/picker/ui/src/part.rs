use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable as _, Pixels, Size, Subscription,
};
use nova_files_api::numbered::stem;
use nova_files_api::{Entry, EntryId, EntryKind, Listing, ListingId, Unreadable};
use nova_files_picker_api::{PickAnswer, PickId, PickKind, PickRequest};
use nova_files_properties_api::{Event as PropertiesEvent, PropertiesId};
use nova_files_shell::text::{TextField, TextFieldEvent};
use nova_files_shell::{Click, ClickAnswer, Host, Location, Navigator, Part, SelectionMade, Slots};

use crate::size::{PICKER, PickerState};
use crate::{Mode, filter, is_plain_name, names_text};

pub struct PickerPart {
    pub(crate) host: Host,
    pub(crate) pick: Option<Picking>,
    size: Option<Size<Pixels>>,
    _selections: Subscription,
}

pub struct Picking {
    pub(crate) request: Box<PickRequest>,
    pub(crate) mode: Mode,
    pub(crate) name: Entity<TextField>,
    pub(crate) conflict: Option<Conflict>,
    pub(crate) jump: Option<Jump>,
    pub(crate) copied: Option<(Vec<EntryId>, String)>,
    pub(crate) placed: Option<OsString>,
    pub(crate) recheck: Option<Recheck>,
    pub(crate) filter: Option<usize>,
    pub(crate) filter_menu: bool,
    pub(crate) quiet: bool,
    _name_events: Subscription,
}

pub struct Conflict {
    pub path: PathBuf,
    pub keep_both: OsString,
}

pub struct Jump {
    pub(crate) fallback: bool,
    pub(crate) name: OsString,
    pub(crate) folder: PathBuf,
}

pub struct Recheck {
    pub(crate) folder: PathBuf,
    pub(crate) name: OsString,
    pub(crate) beside: Option<OsString>,
    pub(crate) probe: Option<PropertiesId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Clicked {
    Open,
    Select,
    Pick,
    Nothing,
}

impl PickerPart {
    pub fn start(&mut self, request: Box<PickRequest>, cx: &mut Context<Self>) {
        let mode = Mode::of(&request.kind);
        let first_filter = filter::first(&request);
        let name = cx.new(TextField::new);
        if let PickKind::Save { name: suggested } = &request.kind {
            let text = suggested.to_string_lossy();
            name.update(cx, |field, cx| {
                field.set_text(&text, cx);
                field.select_range(0..stem(OsStr::new(&*text)).len(), cx);
            });
        }
        let name_events = cx.subscribe(&name, |this, _, event, cx| {
            match event {
                TextFieldEvent::Submitted => this.accept(cx),
                TextFieldEvent::Cancelled => {
                    let asking = this
                        .pick
                        .as_mut()
                        .and_then(|pick| pick.conflict.take())
                        .is_some();
                    if !asking {
                        this.cancel(cx);
                    }
                }
                TextFieldEvent::Changed => {
                    if let Some(pick) = &mut this.pick {
                        pick.conflict = None;
                        pick.recheck = None;
                        pick.jump = None;
                        pick.placed = None;
                        pick.copied = None;
                    }
                }
            }
            cx.notify();
        });
        let placed = match &request.kind {
            PickKind::Save { name } if is_plain_name(name) => Some(name.clone()),
            _ => None,
        };
        self.pick = Some(Picking {
            request,
            mode,
            name,
            conflict: None,
            jump: None,
            copied: None,
            placed,
            recheck: None,
            filter: first_filter,
            filter_menu: false,
            quiet: false,
            _name_events: name_events,
        });
        cx.notify();
    }

    pub fn id(&self) -> Option<PickId> {
        self.pick.as_ref().map(|pick| pick.request.id)
    }

    pub fn name(&self, cx: &App) -> Option<String> {
        let pick = self.pick.as_ref()?;
        Some(pick.name.read(cx).text().to_owned())
    }

    pub fn name_focus(&self, cx: &App) -> Option<FocusHandle> {
        let pick = self.pick.as_ref()?;
        Some(pick.name.read(cx).focus_handle(cx))
    }

    pub fn conflict(&self) -> Option<&Conflict> {
        self.pick.as_ref()?.conflict.as_ref()
    }

    pub fn filter(&self) -> Option<usize> {
        self.pick.as_ref()?.filter
    }

    pub fn wanted(&self) -> Option<EntryKind> {
        Some(match self.pick.as_ref()?.mode {
            Mode::Folders => EntryKind::Folder,
            Mode::Files | Mode::Save => EntryKind::File,
        })
    }

    pub fn several(&self) -> bool {
        self.pick
            .as_ref()
            .is_none_or(|pick| matches!(pick.request.kind, PickKind::Open { multiple: true, .. }))
    }

    pub fn dims(&self, entry: &Entry) -> bool {
        self.pick
            .as_ref()
            .is_some_and(|pick| filter::dims(&pick.request, pick.mode, pick.filter, entry))
    }

    const fn clicked(&self, kind: EntryKind, only: bool, double: bool) -> Clicked {
        let Some(pick) = &self.pick else {
            return Clicked::Nothing;
        };
        match (kind, pick.mode, only) {
            (EntryKind::Folder, Mode::Folders, true) if double => Clicked::Open,
            (EntryKind::Folder, Mode::Folders, _) => Clicked::Select,
            (EntryKind::Folder, _, true) => Clicked::Open,
            (EntryKind::Folder, _, false) | (EntryKind::File, Mode::Folders, _) => Clicked::Nothing,
            (EntryKind::File, Mode::Files, true) if double => Clicked::Pick,
            (EntryKind::File, Mode::Files | Mode::Save, _) => Clicked::Select,
        }
    }

    pub fn pick_entry(&mut self, listing: ListingId, id: EntryId, cx: &mut Context<Self>) -> bool {
        if self.waiting(cx) || self.pick.as_ref().map(|pick| pick.mode) != Some(Mode::Files) {
            return false;
        }
        let path = {
            let entries = self.host.entries.read(cx);
            entries
                .listing()
                .filter(|shown| shown.id == listing)
                .and_then(|shown| entry_at(&shown.entries, id))
                .filter(|entry| entry.kind == EntryKind::File)
                .and_then(|entry| Some(self.folder_of(cx)?.join(&entry.name)))
        };
        path.is_some_and(|path| self.answer_all(vec![path], cx))
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        drop(self.pick.take());
        self.host.nav.update(cx, Navigator::close);
    }

    pub fn answer(&mut self, path: PathBuf, cx: &mut Context<Self>) -> bool {
        self.answer_all(vec![path], cx)
    }

    pub(crate) fn answer_all(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) -> bool {
        let Some(pick) = self.pick.take() else {
            return false;
        };
        let filter = pick.filter;
        pick.request
            .reply
            .answer(PickAnswer::Picked { paths, filter });
        self.host.nav.update(cx, Navigator::close);
        true
    }

    pub(crate) fn waiting(&self, cx: &App) -> bool {
        self.host.entries.read(cx).asked().is_some()
    }

    pub(crate) fn folder_of(&self, cx: &App) -> Option<PathBuf> {
        let entries = self.host.entries.read(cx);
        entries
            .location()
            .and_then(Location::folder)
            .map(Path::to_path_buf)
    }

    pub(crate) fn folder(&self, cx: &App) -> PathBuf {
        self.folder_of(cx).unwrap_or_else(|| self.host.home.clone())
    }

    pub(crate) fn go(&self, path: PathBuf, cx: &mut Context<Self>) {
        self.host
            .nav
            .update(cx, |nav, cx| nav.go(Location::Folder(path), cx));
    }

    pub(crate) fn selection_changed(&mut self, cx: &mut Context<Self>) {
        let (listing, ids) = {
            let entries = self.host.entries.read(cx);
            let ids: Vec<EntryId> = entries.selected().iter().map(|entry| entry.id).collect();
            (entries.listing().cloned(), ids)
        };
        let Some(pick) = &mut self.pick else { return };
        if std::mem::take(&mut pick.quiet) || pick.mode == Mode::Folders {
            return;
        }
        let entries = listing
            .as_ref()
            .map(|shown| &*shown.entries)
            .unwrap_or_default();
        let files: Vec<(EntryId, String)> = ids
            .iter()
            .filter_map(|&id| entry_at(entries, id))
            .filter(|entry| entry.kind == EntryKind::File)
            .map(|entry| (entry.id, entry.name.to_string_lossy().into_owned()))
            .collect();
        if files.is_empty() && !ids.is_empty() {
            return;
        }
        let names: Vec<String> = files.iter().map(|(_, name)| name.clone()).collect();
        let text = names_text(&names);
        pick.name.update(cx, |field, cx| field.set_text(&text, cx));
        pick.copied =
            (!files.is_empty()).then(|| (files.into_iter().map(|(id, _)| id).collect(), text));
        pick.conflict = None;
        pick.recheck = None;
        pick.jump = None;
        cx.notify();
    }

    fn selectable(&self, entry: &Entry, _: &App) -> bool {
        self.wanted().is_none_or(|wanted| entry.kind == wanted)
    }

    fn several_slot(&self, _: &App) -> bool {
        self.several()
    }

    fn dims_slot(&self, entry: &Entry, _: &App) -> bool {
        self.dims(entry)
    }

    fn focus(&self, cx: &App) -> Option<FocusHandle> {
        self.name_focus(cx)
    }

    fn listed(&mut self, listing: &Listing, reordered: bool, cx: &mut Context<Self>) {
        self.folder_opened(listing, reordered, cx);
    }

    fn refused(&mut self, _: ListingId, path: &Path, cx: &mut Context<Self>) {
        self.folder_refused(path, cx);
    }

    fn entry_clicked(
        &mut self,
        listing: ListingId,
        entry: &Entry,
        click: Click,
        cx: &mut Context<Self>,
    ) -> Option<ClickAnswer> {
        self.pick.as_ref()?;
        Some(match self.clicked(entry.kind, click.only, click.double) {
            Clicked::Open => ClickAnswer::Open,
            Clicked::Select => ClickAnswer::Select,
            Clicked::Pick => {
                self.pick_entry(listing, entry.id, cx);
                ClickAnswer::Done
            }
            Clicked::Nothing => ClickAnswer::Done,
        })
    }

    fn peeked(&mut self, event: &PropertiesEvent, cx: &mut Context<Self>) {
        match event {
            PropertiesEvent::Properties { id, .. } => self.probed(*id, false, cx),
            PropertiesEvent::PropertiesUnreadable { id, reason, .. } => {
                self.probed(*id, *reason == Unreadable::Missing, cx);
            }
            PropertiesEvent::SizeMeasured { .. }
            | PropertiesEvent::SpaceMeasured { .. }
            | PropertiesEvent::SpaceUnreadable { .. } => {}
        }
    }

    const fn resized(&mut self, size: Size<Pixels>, _: &mut Context<Self>) {
        if self.pick.is_some() {
            self.size = Some(size);
        }
    }

    fn closing(&mut self, cx: &mut Context<Self>) {
        let Some(value) = self
            .size
            .take()
            .and_then(|size| PickerState::default().with_picker(size).value())
        else {
            return;
        };
        self.host
            .kept
            .update(cx, |kept, _| kept.set(PICKER, &value));
    }
}

pub fn entry_at(entries: &[Entry], id: EntryId) -> Option<&Entry> {
    usize::try_from(id.0)
        .ok()
        .and_then(|index| entries.get(index))
}

impl Part for PickerPart {
    fn open(host: &Host, cx: &mut Context<Self>) -> Self {
        let selections = cx.subscribe(&host.entries, |this, _, _: &SelectionMade, cx| {
            this.selection_changed(cx);
        });
        Self {
            host: host.clone(),
            pick: None,
            size: None,
            _selections: selections,
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.selectable(Self::selectable);
        slots.several(Self::several_slot);
        slots.dims(Self::dims_slot);
        slots.focus(Self::focus);
        slots.listed(Self::listed);
        slots.refused(Self::refused);
        slots.clicked(Self::entry_clicked);
        slots.peek(Self::peeked);
        slots.resized(Self::resized);
        slots.closing(Self::closing);
        slots.started(Self::started);
        slots.dismissed(Self::cancel);
        slots.folder_picks(Self::pick_folder);
        slots.footer(Self::footer);
    }
}
