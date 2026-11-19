use std::any::TypeId;
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{
    Action, AnyElement, App, Context, Div, Entity, FocusHandle, InteractiveElement as _,
    KeyDownEvent, Modifiers, Pixels, ScrollWheelEvent, SharedString, Size, Window,
};
use nova_files_api::{Entry, EntryId, Event, Listing, ListingId, Order};

use crate::assets::Icon;
use nova_files_messages::{Envelope, Message, MessageType};

use crate::command::CommandSpec;
use crate::host::Host;
use crate::jump::Jump;
use crate::layout::Layout;
use crate::location::{Location, PageKind, PageRef};
use crate::menu::{MenuAt, MenuGroup, MenuItem, RowItem, SidebarAt, SidebarRow};
use crate::path_bar::Crumb;
use crate::per_tab::TabId;
use crate::scene::Scene;
use crate::text::TextField;

pub trait Part: Sized + 'static {
    fn open(host: &Host, cx: &mut Context<Self>) -> Self;
    fn slots(slots: &mut Slots<Self>);
}

pub type Applies<P> = fn(&P, &Scene, &App) -> bool;
pub type Items<P> = fn(&P, &Scene, &App) -> Vec<MenuItem>;
pub type Rows<P> = fn(&P, &Scene, &App) -> Vec<SidebarRow>;
pub type Card<P> = fn(&mut P, &Scene, &mut Window, &mut Context<P>) -> Option<AnyElement>;
pub type Cell<P> = fn(&P, &Entry, &App) -> Option<SharedString>;
pub type Columns<P> = fn(&P, &Scene, &App) -> Option<Column<P>>;
pub type Watch<P> = fn(&mut P, &Event, &mut Context<P>);
pub type EmptyNote<P> = fn(&P, &Scene, &App) -> Option<(Icon, SharedString)>;
pub type NameCell<P> = fn(&P, &Entry, &App) -> Option<AnyElement>;
pub type NameField<P> = fn(&P, &Entry, &App) -> Option<Entity<TextField>>;
pub type Stage<P> =
    fn(&mut P, &Scene, Size<Pixels>, &mut Window, &mut Context<P>) -> Option<AnyElement>;
pub type Staging<P> =
    fn(&mut P, &Scene, Size<Pixels>, &mut Window, &mut Context<P>) -> Option<Staged>;

pub struct Staged {
    pub entry: EntryId,
    pub art: AnyElement,
    pub about: SharedString,
}
pub type Busy<P> = fn(&P, &App) -> bool;
pub type RowIcon<P> = fn(&P, &Entry, Pixels, &App) -> Option<AnyElement>;
pub type Opens<P> = fn(&mut P, &Entry, &Path, &mut Context<P>) -> bool;
pub type Closing<P> = fn(&mut P, &mut Context<P>);
pub type Listed<P> = fn(&mut P, &Listing, bool, &mut Context<P>);
pub type DropSlot<P> = Rc<dyn Fn(&mut P, &PageRef, Vec<PathBuf>, &mut Context<P>)>;
pub type DropAim<P> = fn(&P, &DropOver<'_>, &App) -> Option<SharedString>;

#[derive(Debug, Clone, Copy)]
pub struct DropOver<'a> {
    pub at: &'a Location,
    pub name: &'a str,
    pub sources: &'a [PathBuf],
    pub modifiers: Modifiers,
}
pub type RowMenu<P> = fn(&P, &str, &App) -> Vec<Vec<RowItem>>;
pub type Typed<P> = fn(&mut P, &Jump, &Scene, &mut Context<P>) -> bool;
pub type KeyDown<P> = fn(&mut P, &KeyDownEvent, &Scene, &mut Context<P>) -> bool;
pub type Wheel<P> = fn(&mut P, &ScrollWheelEvent, &mut Context<P>) -> bool;
pub type HeadButton<P> = fn(&P, &Scene, &App) -> Option<RowItem>;
pub type DropTake<P> = fn(&mut P, Location, Vec<PathBuf>, Modifiers, &mut Context<P>);
pub type Selectable<P> = fn(&P, &Entry, &App) -> bool;
pub type Several<P> = fn(&P, &App) -> bool;
pub type Dims<P> = fn(&P, &Entry, &App) -> bool;
pub type Focus<P> = fn(&P, &App) -> Option<FocusHandle>;
pub type RowsDrawn<P> = fn(&mut P, ListingId, Range<usize>, &mut Context<P>);
pub type Rescaled<P> = fn(&mut P, f32, &mut Context<P>);
pub type ThumbnailSide<P> = fn(&mut P, u32, &mut Context<P>);
pub type Used<P> = fn(&mut P, &Path, &mut Context<P>);
pub type PageTurn<P> = Rc<dyn Fn(&mut P, &PageRef, TabId, &mut Context<P>)>;
pub type Refused<P> = fn(&mut P, ListingId, &Path, &mut Context<P>);
pub type Clicks<P> = fn(&mut P, ListingId, &Entry, Click, &mut Context<P>) -> Option<ClickAnswer>;
pub type Resized<P> = fn(&mut P, Size<Pixels>, &mut Context<P>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Click {
    pub only: bool,
    pub double: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickAnswer {
    Open,
    Select,
    Done,
}

pub struct CardRank;

impl CardRank {
    pub const RECENT: u8 = 10;
    pub const DRIVES: u8 = 20;
    pub const LAST: u8 = u8::MAX;
}

pub struct BarRank;

impl BarRank {
    pub const FIRST: u8 = 0;
    pub const LAST: u8 = u8::MAX;
}

pub struct ChipRank;

impl ChipRank {
    pub const FIRST: u8 = 0;
    pub const LAST: u8 = u8::MAX;
}

pub struct Column<P> {
    pub head: SharedString,
    pub selector: &'static str,
    pub cell: Cell<P>,
}

type EventSlot<P> = Box<dyn Fn(&mut P, &Envelope, &mut Context<P>)>;
type PeekSlot<P> = (MessageType, EventSlot<P>);
type Bind<P> = Box<dyn Fn(Div, &Entity<P>, &Host) -> Div>;
pub type ListSlot = Rc<dyn Fn(&PageRef, ListingId, Order) -> Option<Envelope>>;
pub type CrumbSlot<P> = Rc<dyn Fn(&P, &PageRef, &App) -> Option<Vec<Crumb>>>;
pub type FolderLook<P> = fn(&P, &Path, &App) -> Option<(SharedString, Icon)>;
pub type FolderLayout<P> = fn(&P, &Path, &App) -> Option<Layout>;
pub type PageSlot<P> =
    Rc<dyn Fn(&mut P, &PageRef, &mut Window, &mut Context<P>) -> Option<AnyElement>>;

pub struct CommandSlot<P> {
    pub(crate) spec: &'static CommandSpec,
    pub(crate) applies: Applies<P>,
    pub(crate) bind: Bind<P>,
}

pub struct Slots<P> {
    pub(crate) events: HashMap<MessageType, EventSlot<P>>,
    pub(crate) peeks: Vec<PeekSlot<P>>,
    pub(crate) commands: Vec<CommandSlot<P>>,
    pub(crate) befores: Vec<Bind<P>>,
    pub(crate) menus: Vec<(MenuAt, MenuGroup, u8, Items<P>)>,
    pub(crate) pages: Vec<(TypeId, PageSlot<P>)>,
    pub(crate) listing_pages: Vec<(TypeId, ListSlot)>,
    pub(crate) crumbs: Vec<CrumbSlot<P>>,
    pub(crate) folder_looks: Vec<FolderLook<P>>,
    pub(crate) layouts: Vec<FolderLayout<P>>,
    pub(crate) columns: Vec<Columns<P>>,
    pub(crate) sidebar: Vec<(SidebarAt, Rows<P>)>,
    pub(crate) start_cards: Vec<(u8, Card<P>)>,
    pub(crate) watches: Vec<Watch<P>>,
    pub(crate) empty_notes: Vec<EmptyNote<P>>,
    pub(crate) sends: Vec<MessageType>,
    pub(crate) name_cells: Vec<NameCell<P>>,
    pub(crate) name_fields: Vec<NameField<P>>,
    pub(crate) busy: Vec<Busy<P>>,
    pub(crate) stages: Vec<Stage<P>>,
    pub(crate) stagings: Vec<Staging<P>>,
    pub(crate) listed: Vec<Listed<P>>,
    pub(crate) drops: Vec<(TypeId, DropSlot<P>)>,
    pub(crate) drop_targets: Vec<(DropAim<P>, DropTake<P>)>,
    pub(crate) row_menus: Vec<RowMenu<P>>,
    pub(crate) head_buttons: Vec<(SidebarAt, &'static str, HeadButton<P>)>,
    pub(crate) bars: Vec<(u8, bool, Card<P>)>,
    pub(crate) footers: Vec<Card<P>>,
    pub(crate) overlays: Vec<Card<P>>,
    pub(crate) layers: Vec<Card<P>>,
    pub(crate) path_chips: Vec<(u8, Card<P>)>,
    pub(crate) list_views: Vec<(Applies<P>, Card<P>)>,
    pub(crate) row_icons: Vec<RowIcon<P>>,
    pub(crate) opens: Vec<Opens<P>>,
    pub(crate) typed: Vec<Typed<P>>,
    pub(crate) key_downs: Vec<KeyDown<P>>,
    pub(crate) wheels: Vec<Wheel<P>>,
    pub(crate) closing: Vec<Closing<P>>,
    pub(crate) clicks: Vec<Clicks<P>>,
    pub(crate) resized: Vec<Resized<P>>,
    pub(crate) selectable: Vec<Selectable<P>>,
    pub(crate) several: Vec<Several<P>>,
    pub(crate) dims: Vec<Dims<P>>,
    pub(crate) focus: Vec<Focus<P>>,
    pub(crate) refused: Vec<Refused<P>>,
    pub(crate) used: Vec<Used<P>>,
    pub(crate) rows_drawn: Vec<RowsDrawn<P>>,
    pub(crate) rescaled: Vec<Rescaled<P>>,
    pub(crate) thumbnail_sides: Vec<ThumbnailSide<P>>,
    pub(crate) pages_shown: Vec<PageTurn<P>>,
    pub(crate) pages_left: Vec<PageTurn<P>>,
}

impl<P> Default for Slots<P> {
    fn default() -> Self {
        Self {
            events: HashMap::new(),
            peeks: Vec::new(),
            commands: Vec::new(),
            befores: Vec::new(),
            menus: Vec::new(),
            pages: Vec::new(),
            listing_pages: Vec::new(),
            crumbs: Vec::new(),
            folder_looks: Vec::new(),
            layouts: Vec::new(),
            columns: Vec::new(),
            sidebar: Vec::new(),
            start_cards: Vec::new(),
            watches: Vec::new(),
            empty_notes: Vec::new(),
            sends: Vec::new(),
            name_cells: Vec::new(),
            name_fields: Vec::new(),
            busy: Vec::new(),
            stages: Vec::new(),
            stagings: Vec::new(),
            listed: Vec::new(),
            drops: Vec::new(),
            drop_targets: Vec::new(),
            row_menus: Vec::new(),
            head_buttons: Vec::new(),
            bars: Vec::new(),
            footers: Vec::new(),
            overlays: Vec::new(),
            layers: Vec::new(),
            path_chips: Vec::new(),
            list_views: Vec::new(),
            row_icons: Vec::new(),
            opens: Vec::new(),
            typed: Vec::new(),
            key_downs: Vec::new(),
            wheels: Vec::new(),
            closing: Vec::new(),
            clicks: Vec::new(),
            resized: Vec::new(),
            selectable: Vec::new(),
            several: Vec::new(),
            dims: Vec::new(),
            focus: Vec::new(),
            refused: Vec::new(),
            used: Vec::new(),
            rows_drawn: Vec::new(),
            rescaled: Vec::new(),
            thumbnail_sides: Vec::new(),
            pages_shown: Vec::new(),
            pages_left: Vec::new(),
        }
    }
}

impl<P: Part> Slots<P> {
    pub fn on_event<E: Message + Clone>(&mut self, take: fn(&mut P, E, &mut Context<P>)) {
        self.events.insert(
            MessageType::of::<E>(),
            Box::new(move |part, envelope, cx| {
                if let Some(event) = envelope.get::<E>() {
                    take(part, event.clone(), cx);
                }
            }),
        );
    }

    pub fn peek<E: Message>(&mut self, see: fn(&mut P, &E, &mut Context<P>)) {
        self.peeks.push((
            MessageType::of::<E>(),
            Box::new(move |part, envelope, cx| {
                if let Some(event) = envelope.get::<E>() {
                    see(part, event, cx);
                }
            }),
        ));
    }

    pub fn command<A: Action>(
        &mut self,
        spec: &'static CommandSpec,
        applies: Applies<P>,
        run: fn(&mut P, &A, &mut Window, &mut Context<P>),
    ) {
        self.commands.push(CommandSlot {
            spec,
            applies,
            bind: Box::new(move |root, entity, host| {
                let (entity, host) = (entity.clone(), host.clone());
                root.on_action(move |action: &A, window, cx| {
                    let scene = host.scene(cx);
                    if applies(entity.read(cx), &scene, cx) {
                        entity.update(cx, |part, cx| run(part, action, window, cx));
                    }
                })
            }),
        });
    }

    pub fn before<A: Action>(
        &mut self,
        take: fn(&mut P, &A, &mut Window, &mut Context<P>) -> bool,
    ) {
        self.befores.push(Box::new(move |root, entity, _| {
            let entity = entity.clone();
            root.capture_action(move |action: &A, window, cx| {
                if entity.update(cx, |part, cx| take(part, action, window, cx)) {
                    cx.stop_propagation();
                }
            })
        }));
    }

    pub fn menu(&mut self, at: MenuAt, items: Items<P>) {
        self.menu_in(at, MenuGroup::Part, 0, items);
    }

    pub fn menu_in(&mut self, at: MenuAt, group: MenuGroup, rank: u8, items: Items<P>) {
        self.menus.push((at, group, rank, items));
    }

    pub fn page<K: PageKind>(
        &mut self,
        render: fn(&mut P, &K, &mut Window, &mut Context<P>) -> AnyElement,
    ) {
        let slot: PageSlot<P> = Rc::new(move |part, page, window, cx| {
            page.get::<K>().map(|page| render(part, page, window, cx))
        });
        self.pages.push((TypeId::of::<K>(), slot));
    }

    pub fn listing_page<K: PageKind>(&mut self, list: fn(&K, ListingId, Order) -> Envelope) {
        let slot: ListSlot =
            Rc::new(move |page, id, order| page.get::<K>().map(|page| list(page, id, order)));
        self.listing_pages.push((TypeId::of::<K>(), slot));
    }

    pub fn crumbs<K: PageKind>(&mut self, crumbs: fn(&P, &K, &App) -> Vec<Crumb>) {
        let slot: CrumbSlot<P> =
            Rc::new(move |part, page, cx| page.get::<K>().map(|page| crumbs(part, page, cx)));
        self.crumbs.push(slot);
    }

    pub fn folder_look(&mut self, look: FolderLook<P>) {
        self.folder_looks.push(look);
    }

    pub fn layout(&mut self, layout: FolderLayout<P>) {
        self.layouts.push(layout);
    }

    pub fn column(&mut self, column: Columns<P>) {
        self.columns.push(column);
    }

    pub fn sidebar(&mut self, at: SidebarAt, rows: Rows<P>) {
        self.sidebar.push((at, rows));
    }

    pub fn start_card(&mut self, card: Card<P>) {
        self.start_card_at(CardRank::LAST, card);
    }

    pub fn start_card_at(&mut self, rank: u8, card: Card<P>) {
        self.start_cards.push((rank, card));
    }

    pub fn watch(&mut self, watch: Watch<P>) {
        self.watches.push(watch);
    }

    pub fn empty_note(&mut self, note: EmptyNote<P>) {
        self.empty_notes.push(note);
    }

    pub fn name_cell(&mut self, cell: NameCell<P>) {
        self.name_cells.push(cell);
    }

    pub fn name_field(&mut self, field: NameField<P>) {
        self.name_fields.push(field);
    }

    pub fn busy(&mut self, busy: Busy<P>) {
        self.busy.push(busy);
    }

    pub fn stage(&mut self, stage: Stage<P>) {
        self.stages.push(stage);
    }

    pub fn staged(&mut self, staged: Staging<P>) {
        self.stagings.push(staged);
    }

    pub fn listed(&mut self, listed: Listed<P>) {
        self.listed.push(listed);
    }

    pub fn drops<K: PageKind>(&mut self, take: fn(&mut P, &K, Vec<PathBuf>, &mut Context<P>)) {
        let slot: DropSlot<P> = Rc::new(move |part, page, paths, cx| {
            if let Some(page) = page.get::<K>() {
                take(part, page, paths, cx);
            }
        });
        self.drops.push((TypeId::of::<K>(), slot));
    }

    pub fn row_menu(&mut self, items: RowMenu<P>) {
        self.row_menus.push(items);
    }

    pub fn section_button(&mut self, at: SidebarAt, id: &'static str, button: HeadButton<P>) {
        self.head_buttons.push((at, id, button));
    }

    pub fn drop_target(&mut self, aim: DropAim<P>, take: DropTake<P>) {
        self.drop_targets.push((aim, take));
    }

    pub fn bar(&mut self, bar: Card<P>) {
        self.bars.push((BarRank::LAST, false, bar));
    }

    pub fn shrinking_bar_at(&mut self, rank: u8, bar: Card<P>) {
        self.bars.push((rank, true, bar));
    }

    pub fn footer(&mut self, footer: Card<P>) {
        self.footers.push(footer);
    }

    pub fn overlay(&mut self, overlay: Card<P>) {
        self.overlays.push(overlay);
    }

    pub fn layer(&mut self, layer: Card<P>) {
        self.layers.push(layer);
    }

    pub fn path_chip(&mut self, chip: Card<P>) {
        self.path_chip_at(ChipRank::LAST, chip);
    }

    pub fn path_chip_at(&mut self, rank: u8, chip: Card<P>) {
        self.path_chips.push((rank, chip));
    }

    pub fn list_view(&mut self, applies: Applies<P>, draw: Card<P>) {
        self.list_views.push((applies, draw));
    }

    pub fn row_icon(&mut self, icon: RowIcon<P>) {
        self.row_icons.push(icon);
    }

    pub fn opens(&mut self, opens: Opens<P>) {
        self.opens.push(opens);
    }

    pub fn typed(&mut self, typed: Typed<P>) {
        self.typed.push(typed);
    }

    pub fn key_down(&mut self, key_down: KeyDown<P>) {
        self.key_downs.push(key_down);
    }

    pub fn wheel(&mut self, wheel: Wheel<P>) {
        self.wheels.push(wheel);
    }

    pub fn closing(&mut self, closing: Closing<P>) {
        self.closing.push(closing);
    }

    pub fn clicked(&mut self, clicked: Clicks<P>) {
        self.clicks.push(clicked);
    }

    pub fn resized(&mut self, resized: Resized<P>) {
        self.resized.push(resized);
    }

    pub fn selectable(&mut self, selectable: Selectable<P>) {
        self.selectable.push(selectable);
    }

    pub fn several(&mut self, several: Several<P>) {
        self.several.push(several);
    }

    pub fn dims(&mut self, dims: Dims<P>) {
        self.dims.push(dims);
    }

    pub fn focus(&mut self, focus: Focus<P>) {
        self.focus.push(focus);
    }

    pub fn refused(&mut self, refused: Refused<P>) {
        self.refused.push(refused);
    }

    pub fn used(&mut self, used: Used<P>) {
        self.used.push(used);
    }

    pub fn rows_drawn(&mut self, drawn: RowsDrawn<P>) {
        self.rows_drawn.push(drawn);
    }

    pub fn rescaled(&mut self, rescaled: Rescaled<P>) {
        self.rescaled.push(rescaled);
    }

    pub fn thumbnail_side(&mut self, changed: ThumbnailSide<P>) {
        self.thumbnail_sides.push(changed);
    }

    pub fn page_shown<K: PageKind>(&mut self, shown: fn(&mut P, &K, TabId, &mut Context<P>)) {
        self.pages_shown.push(page_turn(shown));
    }

    pub fn page_left<K: PageKind>(&mut self, left: fn(&mut P, &K, TabId, &mut Context<P>)) {
        self.pages_left.push(page_turn(left));
    }

    pub fn sends<M: Message>(&mut self) {
        self.sends.push(MessageType::of::<M>());
    }

    pub fn takes(&self) -> Vec<MessageType> {
        self.events.keys().copied().collect()
    }

    pub fn sent(&self) -> Vec<MessageType> {
        self.sends.clone()
    }

    pub fn specs(&self) -> Vec<&'static CommandSpec> {
        self.commands.iter().map(|command| command.spec).collect()
    }
}

fn page_turn<P: 'static, K: PageKind>(turn: fn(&mut P, &K, TabId, &mut Context<P>)) -> PageTurn<P> {
    Rc::new(move |part, page, tab, cx| {
        if let Some(page) = page.get::<K>() {
            turn(part, page, tab, cx);
        }
    })
}
