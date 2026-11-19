use std::ffi::{OsStr, OsString};
use std::ops::Range;
use std::path::PathBuf;
use std::time::Instant;

use gpui::{Context, EventEmitter};
use nova_files_api::{Entry, EntryId, Listing, ListingId};

use crate::cursor::Move;
use crate::jump::PAUSE;
use crate::location::Location;
use crate::over::{LaidOver, OverLeft, OverWant};
use crate::scene::Showing;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Laid {
    #[default]
    Off,
    Over,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Shown {
    pub location: Option<Location>,
    pub listing: Option<Listing>,
    pub selected: Vec<EntryId>,
    pub picker: bool,
    pub asked: Option<ListingId>,
    pub laid: Laid,
    pub gathered: bool,
    pub cursor: Option<usize>,
    pub showing: Showing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntriesAsked;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionMade;

#[derive(Debug, Default)]
pub struct Entries {
    shown: Shown,
    selects: Option<Vec<OsString>>,
    selects_later: Option<(PathBuf, Vec<OsString>)>,
    drawn: Range<usize>,
    failed: Option<ListingId>,
    scale_factor: f32,
    thumbnail_side: u32,
    jumped: Option<Instant>,
    moves: Option<(Option<usize>, Move)>,
    opens: Option<EntryId>,
    over: Option<OverWant>,
}

impl EventEmitter<EntriesAsked> for Entries {}
impl EventEmitter<SelectionMade> for Entries {}

impl EventEmitter<OverLeft> for Entries {}

impl Entries {
    pub const fn location(&self) -> Option<&Location> {
        self.shown.location.as_ref()
    }

    pub const fn listing(&self) -> Option<&Listing> {
        self.shown.listing.as_ref()
    }

    pub const fn picker(&self) -> bool {
        self.shown.picker
    }

    pub fn searching(&self) -> bool {
        self.shown.laid != Laid::Off
    }

    pub fn over_failed(&self) -> bool {
        self.shown.laid == Laid::Failed
    }

    pub fn lay_over(&mut self, over: LaidOver, cx: &mut Context<Self>) {
        self.over = Some(OverWant::Lay(over));
        cx.emit(EntriesAsked);
    }

    pub fn take_back(&mut self, cx: &mut Context<Self>) {
        self.over = Some(OverWant::TakeBack);
        cx.emit(EntriesAsked);
    }

    pub const fn take_over_want(&mut self) -> Option<OverWant> {
        self.over.take()
    }

    pub fn over_left(&mut self, cx: &mut Context<Self>) {
        cx.emit(OverLeft);
    }

    pub const fn gathered(&self) -> bool {
        self.shown.gathered
    }

    pub const fn cursor(&self) -> Option<usize> {
        self.shown.cursor
    }

    pub const fn showing(&self) -> Showing {
        self.shown.showing
    }

    pub fn jumping(&self, now: Instant) -> bool {
        self.jumped
            .is_some_and(|jumped| now.saturating_duration_since(jumped) < PAUSE)
    }

    pub const fn set_jumped(&mut self, jumped: Option<Instant>) {
        self.jumped = jumped;
    }

    pub fn move_cursor(&mut self, from: Option<usize>, step: Move, cx: &mut Context<Self>) {
        self.moves = Some((from, step));
        cx.emit(EntriesAsked);
    }

    pub const fn take_move(&mut self) -> Option<(Option<usize>, Move)> {
        self.moves.take()
    }

    pub fn open(&mut self, id: EntryId, cx: &mut Context<Self>) {
        self.opens = Some(id);
        cx.emit(EntriesAsked);
    }

    pub const fn take_open(&mut self) -> Option<EntryId> {
        self.opens.take()
    }

    pub const fn asked(&self) -> Option<ListingId> {
        self.shown.asked
    }

    pub const fn selected_count(&self) -> usize {
        self.shown.selected.len()
    }

    pub fn selected_ids(&self) -> &[EntryId] {
        &self.shown.selected
    }

    pub fn selected(&self) -> Vec<&Entry> {
        let Some(listing) = &self.shown.listing else {
            return Vec::new();
        };
        self.shown
            .selected
            .iter()
            .filter_map(|id| {
                usize::try_from(id.0)
                    .ok()
                    .and_then(|at| listing.entries.get(at))
            })
            .collect()
    }

    pub fn selected_paths(&self) -> Vec<PathBuf> {
        let Some(listing) = &self.shown.listing else {
            return Vec::new();
        };
        self.selected()
            .into_iter()
            .map(|entry| listing.path_of(entry))
            .collect()
    }

    pub fn drawn(&self, name: &OsStr) -> bool {
        self.shown.listing.as_ref().is_some_and(|listing| {
            listing
                .entries
                .iter()
                .position(|entry| entry.name == name)
                .is_some_and(|at| self.drawn.contains(&at))
        })
    }

    pub const fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    pub const fn set_scale_factor(&mut self, scale_factor: f32) {
        self.scale_factor = scale_factor;
    }

    pub const fn thumbnail_side(&self) -> u32 {
        self.thumbnail_side
    }

    pub const fn set_thumbnail_side(&mut self, side: u32) {
        self.thumbnail_side = side;
    }

    pub const fn set_drawn(&mut self, drawn: Range<usize>) {
        self.drawn = drawn;
    }

    pub fn selection_made(&mut self, cx: &mut Context<Self>) {
        cx.emit(SelectionMade);
    }

    pub fn show(&mut self, shown: Shown, cx: &mut Context<Self>) {
        if self.shown != shown {
            self.shown = shown;
            cx.notify();
        }
    }

    pub fn select(&mut self, names: Vec<OsString>, cx: &mut Context<Self>) {
        self.selects = Some(names);
        cx.emit(EntriesAsked);
    }

    pub const fn take_select(&mut self) -> Option<Vec<OsString>> {
        self.selects.take()
    }

    pub fn listing_failed(&mut self, id: ListingId, cx: &mut Context<Self>) {
        self.failed = Some(id);
        cx.emit(EntriesAsked);
    }

    pub const fn take_failed(&mut self) -> Option<ListingId> {
        self.failed.take()
    }

    pub fn select_when_listed(
        &mut self,
        folder: PathBuf,
        names: Vec<OsString>,
        cx: &mut Context<Self>,
    ) {
        self.selects_later = Some((folder, names));
        cx.emit(EntriesAsked);
    }

    pub const fn take_select_later(&mut self) -> Option<(PathBuf, Vec<OsString>)> {
        self.selects_later.take()
    }
}
