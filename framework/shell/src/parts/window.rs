use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyElement, AnyView, App, Div, Entity, FocusHandle, KeyDownEvent, Pixels, RenderImage,
    ScrollEvent, SharedString, Size, Subscription, Window,
};
use nova_files_api::{Entry, Event, Listing, ListingId, Order, Question};
use nova_files_messages::Envelope;

use super::{OpenPart, Opened, ShownColumn, Turn};
use crate::assets::Icon;
use crate::command::CommandSpec;
use crate::island::job_words::Words;
use crate::jump::Jump;
use crate::layout::Layout;
use crate::location::PageRef;
use crate::menu::{MenuAt, Placed, RowItem, SectionButton, SidebarAt, SidebarRow};
use crate::navigator::FolderPick;
use crate::opening::Opening;
use crate::part::Part;
use crate::part::{Asking, Click, ClickAnswer, DropOver, Staged};
use crate::path_bar::Crumb;
use crate::per_tab::TabId;
use crate::scene::Scene;
use crate::text::TextField;

#[derive(Default)]
pub struct WindowParts {
    pub(super) opened: Vec<Box<dyn OpenPart>>,
    pub(super) words: Words,
}

impl WindowParts {
    pub const fn words(&self) -> &Words {
        &self.words
    }

    pub fn peek(&self, envelope: &Envelope, cx: &mut App) {
        for part in &self.opened {
            part.peek(envelope, cx);
        }
    }

    pub fn deliver(&self, envelope: Envelope, cx: &mut App) -> Result<(), Envelope> {
        let mut taken = false;
        for part in &self.opened {
            taken |= part.deliver(&envelope, cx);
        }
        if taken { Ok(()) } else { Err(envelope) }
    }

    pub fn bind(&self, root: Div) -> Div {
        self.opened.iter().fold(root, |root, part| part.bind(root))
    }

    pub fn applies(&self, spec: &CommandSpec, scene: &Scene, cx: &App) -> bool {
        scene.rules.meets(spec.needs)
            && self
                .opened
                .iter()
                .find_map(|part| part.applies(spec, scene, cx))
                .unwrap_or(true)
    }

    pub fn menu(&self, at: MenuAt, scene: &Scene, cx: &App) -> Vec<Placed> {
        self.opened
            .iter()
            .flat_map(|part| part.menu(at, scene, cx))
            .map(|mut placed| {
                placed
                    .items
                    .retain(|item| self.applies(item.run, scene, cx));
                placed
            })
            .filter(|placed| !placed.items.is_empty())
            .collect()
    }

    pub fn page(&self, page: &PageRef) -> Option<AnyView> {
        self.opened.iter().find_map(|part| part.page(page))
    }

    pub fn crumbs(&self, page: &PageRef, cx: &App) -> Option<Vec<Crumb>> {
        self.opened.iter().find_map(|part| part.crumbs(page, cx))
    }

    pub fn folder_look(&self, folder: &Path, cx: &App) -> Option<(SharedString, Icon)> {
        self.opened
            .iter()
            .find_map(|part| part.folder_look(folder, cx))
    }

    pub fn layout(&self, folder: &Path, cx: &App) -> Option<Layout> {
        self.opened.iter().find_map(|part| part.layout(folder, cx))
    }

    pub fn column(&self, scene: &Scene, cx: &App) -> Option<ShownColumn> {
        self.opened.iter().find_map(|part| part.column(scene, cx))
    }

    pub fn sidebar(&self, at: SidebarAt, scene: &Scene, cx: &App) -> Vec<SidebarRow> {
        self.opened
            .iter()
            .flat_map(|part| part.sidebar(at, scene, cx))
            .collect()
    }

    pub fn start_cards(&self) -> Vec<AnyView> {
        let mut cards: Vec<(u8, AnyView)> = self
            .opened
            .iter()
            .flat_map(|part| part.start_cards())
            .collect();
        cards.sort_by_key(|(rank, _)| *rank);
        cards.into_iter().map(|(_, card)| card).collect()
    }

    pub fn watch(&self, event: &Event, cx: &mut App) {
        for part in &self.opened {
            part.watch(event, cx);
        }
    }

    pub fn empty_note(&self, scene: &Scene, cx: &App) -> Option<(Icon, SharedString)> {
        self.opened
            .iter()
            .find_map(|part| part.empty_note(scene, cx))
    }

    pub fn name_cell(&self, entry: &Entry, cx: &App) -> Option<AnyElement> {
        self.opened
            .iter()
            .find_map(|part| part.name_cell(entry, cx))
    }

    pub fn name_field(&self, entry: &Entry, cx: &App) -> Option<Entity<TextField>> {
        self.opened
            .iter()
            .find_map(|part| part.name_field(entry, cx))
    }

    pub fn busy(&self, cx: &App) -> bool {
        self.opened.iter().any(|part| part.busy(cx))
    }

    pub fn lists(&self, page: &PageRef) -> bool {
        self.opened.iter().any(|part| part.lists(page))
    }

    pub fn list_page(&self, page: &PageRef, id: ListingId, order: Order) -> Option<Envelope> {
        self.opened
            .iter()
            .find_map(|part| part.list_page(page, id, order))
    }

    pub fn stage(
        &self,
        scene: &Scene,
        room: Size<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        self.opened
            .iter()
            .find_map(|part| part.stage(scene, room, window, cx))
    }

    pub fn staged(
        &self,
        scene: &Scene,
        room: Size<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Staged> {
        self.opened
            .iter()
            .find_map(|part| part.staged(scene, room, window, cx))
    }

    pub fn listed(&self, listing: &Listing, reordered: bool, cx: &mut App) {
        for part in &self.opened {
            part.listed(listing, reordered, cx);
        }
    }

    pub fn row_menu(&self, row: &str, cx: &App) -> Vec<Vec<RowItem>> {
        self.opened
            .iter()
            .flat_map(|part| part.row_menu(row, cx))
            .filter(|group| !group.is_empty())
            .collect()
    }

    pub fn section_buttons(
        &self,
        at: SidebarAt,
        scene: &Scene,
        cx: &App,
    ) -> Vec<(&'static str, SectionButton)> {
        self.opened
            .iter()
            .flat_map(|part| part.section_buttons(at, scene, cx))
            .collect()
    }

    pub fn drop_label(&self, over: &DropOver<'_>, cx: &App) -> Option<SharedString> {
        self.opened
            .iter()
            .find_map(|part| part.drop_label(over, cx))
    }

    pub fn drop_at(&self, over: &DropOver<'_>, cx: &mut App) -> bool {
        self.opened.iter().any(|part| part.drop_at(over, cx))
    }

    pub fn takes_drops(&self, page: &PageRef) -> bool {
        self.opened.iter().any(|part| part.takes_drops(page))
    }

    pub fn drop_on(&self, page: &PageRef, mut paths: Vec<PathBuf>, cx: &mut App) {
        for part in &self.opened {
            match part.drop_on(page, paths, cx) {
                Ok(()) => return,
                Err(back) => paths = back,
            }
        }
    }

    pub fn bars(&self) -> Vec<AnyView> {
        let mut bars: Vec<(u8, AnyView)> =
            self.opened.iter().flat_map(|part| part.bars()).collect();
        bars.sort_by_key(|(rank, _)| *rank);
        bars.into_iter().map(|(_, bar)| bar).collect()
    }

    pub fn footers(&self) -> Vec<AnyView> {
        self.opened.iter().flat_map(|part| part.footers()).collect()
    }

    pub fn clicked(
        &self,
        listing: ListingId,
        entry: &Entry,
        click: Click,
        cx: &mut App,
    ) -> Option<ClickAnswer> {
        self.opened
            .iter()
            .find_map(|part| part.clicked(listing, entry, click, cx))
    }

    pub fn resized(&self, size: Size<Pixels>, cx: &mut App) {
        for part in &self.opened {
            part.resized(size, cx);
        }
    }

    pub fn started(&self, opening: &Opening, window: &mut Window, cx: &mut App) {
        for part in &self.opened {
            part.started(opening, window, cx);
        }
    }

    pub fn dismissed(&self, cx: &mut App) {
        for part in &self.opened {
            part.dismissed(cx);
        }
    }

    pub fn pick_folder(&self, pick: &FolderPick, folder: Option<&Path>, cx: &mut App) -> bool {
        self.opened
            .iter()
            .any(|part| part.pick_folder(pick, folder, cx))
    }

    pub fn overlays(&self) -> Vec<AnyView> {
        self.opened
            .iter()
            .flat_map(|part| part.overlays())
            .collect()
    }

    pub fn layers(&self) -> Vec<AnyView> {
        self.opened.iter().flat_map(|part| part.layers()).collect()
    }

    pub fn path_chips(&self) -> Vec<(u8, AnyView)> {
        let mut chips: Vec<(u8, AnyView)> = self
            .opened
            .iter()
            .flat_map(|part| part.path_chips())
            .collect();
        chips.sort_by_key(|(rank, _)| *rank);
        chips
    }

    pub fn list_view(&self, scene: &Scene, cx: &App) -> Option<AnyView> {
        self.opened
            .iter()
            .find_map(|part| part.list_view(scene, cx))
    }

    pub fn asked(&self, asking: Option<Asking<'_>>, window: &mut Window, cx: &mut App) {
        for part in &self.opened {
            part.asked(asking, window, cx);
        }
    }

    pub fn question(&self, question: &'static Question) -> Option<AnyView> {
        self.opened.iter().find_map(|part| part.question(question))
    }

    pub fn island(&self, scene: &Scene, cx: &App) -> Option<AnyView> {
        self.opened.iter().find_map(|part| part.island(scene, cx))
    }

    pub fn row_icon(&self, entry: &Entry, side: Pixels, cx: &App) -> Option<AnyElement> {
        self.opened
            .iter()
            .find_map(|part| part.row_icon(entry, side, cx))
    }

    pub fn row_mark(&self, entry: &Entry, cx: &App) -> Option<AnyElement> {
        self.opened.iter().find_map(|part| part.row_mark(entry, cx))
    }

    pub fn row_picture(&self, entry: &Entry, cx: &App) -> Option<Arc<RenderImage>> {
        self.opened
            .iter()
            .find_map(|part| part.row_picture(entry, cx))
    }

    pub fn open_entry(&self, entry: &Entry, path: &Path, cx: &mut App) -> bool {
        self.opened
            .iter()
            .any(|part| part.open_entry(entry, path, cx))
    }

    pub fn typed(&self, jump: &Jump, scene: &Scene, cx: &mut App) -> bool {
        self.opened.iter().any(|part| part.typed(jump, scene, cx))
    }

    pub fn key_down(&self, event: &KeyDownEvent, scene: &Scene, cx: &mut App) -> bool {
        self.opened
            .iter()
            .any(|part| part.key_down(event, scene, cx))
    }

    pub fn wheel(&self, event: &ScrollEvent, cx: &mut App) -> bool {
        self.opened.iter().any(|part| part.wheel(event, cx))
    }

    pub fn closing(&self, cx: &mut App) {
        for part in &self.opened {
            part.closing(cx);
        }
    }

    pub fn part<P: Part>(&self) -> Option<Entity<P>> {
        self.opened.iter().find_map(|part| {
            part.as_any()
                .downcast_ref::<Opened<P>>()
                .map(|opened| opened.entity.clone())
        })
    }

    pub fn observe(&self, changed: impl Fn(&mut App) + 'static, cx: &mut App) -> Vec<Subscription> {
        let changed: Rc<dyn Fn(&mut App)> = Rc::new(changed);
        self.opened
            .iter()
            .map(|part| part.observe(Rc::clone(&changed), cx))
            .collect()
    }

    pub fn selectable(&self, entry: &Entry, cx: &App) -> bool {
        self.opened.iter().all(|part| part.selectable(entry, cx))
    }

    pub fn several(&self, cx: &App) -> bool {
        self.opened.iter().all(|part| part.several(cx))
    }

    pub fn dims(&self, entry: &Entry, cx: &App) -> bool {
        self.opened.iter().any(|part| part.dims(entry, cx))
    }

    pub fn focus(&self, cx: &App) -> Option<FocusHandle> {
        self.opened.iter().find_map(|part| part.focus(cx))
    }

    pub fn refused(&self, id: ListingId, path: &Path, cx: &mut App) {
        for part in &self.opened {
            part.refused(id, path, cx);
        }
    }

    pub fn used(&self, folder: &Path, cx: &mut App) {
        for part in &self.opened {
            part.used(folder, cx);
        }
    }

    pub fn rows_drawn(&self, id: ListingId, rows: &Range<usize>, cx: &mut App) {
        for part in &self.opened {
            part.rows_drawn(id, rows, cx);
        }
    }

    pub fn rescaled(&self, scale_factor: f32, cx: &mut App) {
        for part in &self.opened {
            part.rescaled(scale_factor, cx);
        }
    }

    pub fn thumbnail_side(&self, side: u32, cx: &mut App) {
        for part in &self.opened {
            part.thumbnail_side(side, cx);
        }
    }

    pub fn page_turned(&self, page: &PageRef, tab: TabId, turn: Turn, cx: &mut App) {
        for part in &self.opened {
            part.page_turned(page, tab, turn, cx);
        }
    }
}
