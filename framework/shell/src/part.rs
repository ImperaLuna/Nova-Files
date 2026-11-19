use std::any::TypeId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{
    Action, AnyElement, App, Context, Div, Entity, InteractiveElement as _, Modifiers,
    SharedString, Window,
};
use nova_files_api::{Entry, Event, Listing};

use crate::assets::Icon;
use nova_files_messages::{Envelope, Message, MessageType};

use crate::command::CommandSpec;
use crate::host::Host;
use crate::location::{Location, PageKind, PageRef};
use crate::menu::{MenuAt, MenuGroup, MenuItem, RowItem, SidebarAt, SidebarRow};
use crate::scene::Scene;

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
pub type Opens<P> = fn(&mut P, &Entry, &Path, &mut Context<P>) -> bool;
pub type Closing<P> = fn(&mut P, &mut Context<P>);
pub type Listed<P> = fn(&mut P, &Listing, &mut Context<P>);
pub type DropSlot<P> = Rc<dyn Fn(&mut P, &PageRef, Vec<PathBuf>, &mut Context<P>)>;
pub type DropAim<P> = fn(&P, &Location, &[PathBuf], Modifiers, &App) -> Option<SharedString>;
pub type RowMenu<P> = fn(&P, &str, &App) -> Vec<RowItem>;
pub type DropTake<P> = fn(&mut P, Location, Vec<PathBuf>, Modifiers, &mut Context<P>);

pub struct Column<P> {
    pub head: SharedString,
    pub selector: &'static str,
    pub cell: Cell<P>,
}

type EventSlot<P> = Box<dyn Fn(&mut P, &Envelope, &mut Context<P>)>;
type Bind<P> = Box<dyn Fn(Div, &Entity<P>, &Host) -> Div>;
pub type PageSlot<P> =
    Rc<dyn Fn(&mut P, &PageRef, &mut Window, &mut Context<P>) -> Option<AnyElement>>;

pub struct CommandSlot<P> {
    pub(crate) spec: &'static CommandSpec,
    pub(crate) applies: Applies<P>,
    pub(crate) bind: Bind<P>,
}

pub struct Slots<P> {
    pub(crate) events: HashMap<MessageType, EventSlot<P>>,
    pub(crate) commands: Vec<CommandSlot<P>>,
    pub(crate) menus: Vec<(MenuAt, MenuGroup, u8, Items<P>)>,
    pub(crate) pages: Vec<(TypeId, PageSlot<P>)>,
    pub(crate) columns: Vec<Columns<P>>,
    pub(crate) sidebar: Vec<(SidebarAt, Rows<P>)>,
    pub(crate) start_cards: Vec<Card<P>>,
    pub(crate) watches: Vec<Watch<P>>,
    pub(crate) empty_notes: Vec<EmptyNote<P>>,
    pub(crate) sends: Vec<MessageType>,
    pub(crate) name_cells: Vec<NameCell<P>>,
    pub(crate) listed: Vec<Listed<P>>,
    pub(crate) drops: Vec<(TypeId, DropSlot<P>)>,
    pub(crate) drop_targets: Vec<(DropAim<P>, DropTake<P>)>,
    pub(crate) row_menus: Vec<RowMenu<P>>,
    pub(crate) bars: Vec<Card<P>>,
    pub(crate) overlays: Vec<Card<P>>,
    pub(crate) list_views: Vec<(Applies<P>, Card<P>)>,
    pub(crate) row_icons: Vec<NameCell<P>>,
    pub(crate) opens: Vec<Opens<P>>,
    pub(crate) closing: Vec<Closing<P>>,
}

impl<P> Default for Slots<P> {
    fn default() -> Self {
        Self {
            events: HashMap::new(),
            commands: Vec::new(),
            menus: Vec::new(),
            pages: Vec::new(),
            columns: Vec::new(),
            sidebar: Vec::new(),
            start_cards: Vec::new(),
            watches: Vec::new(),
            empty_notes: Vec::new(),
            sends: Vec::new(),
            name_cells: Vec::new(),
            listed: Vec::new(),
            drops: Vec::new(),
            drop_targets: Vec::new(),
            row_menus: Vec::new(),
            bars: Vec::new(),
            overlays: Vec::new(),
            list_views: Vec::new(),
            row_icons: Vec::new(),
            opens: Vec::new(),
            closing: Vec::new(),
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

    pub fn column(&mut self, column: Columns<P>) {
        self.columns.push(column);
    }

    pub fn sidebar(&mut self, at: SidebarAt, rows: Rows<P>) {
        self.sidebar.push((at, rows));
    }

    pub fn start_card(&mut self, card: Card<P>) {
        self.start_cards.push(card);
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

    pub fn drop_target(&mut self, aim: DropAim<P>, take: DropTake<P>) {
        self.drop_targets.push((aim, take));
    }

    pub fn bar(&mut self, bar: Card<P>) {
        self.bars.push(bar);
    }

    pub fn overlay(&mut self, overlay: Card<P>) {
        self.overlays.push(overlay);
    }

    pub fn list_view(&mut self, applies: Applies<P>, draw: Card<P>) {
        self.list_views.push((applies, draw));
    }

    pub fn row_icon(&mut self, icon: NameCell<P>) {
        self.row_icons.push(icon);
    }

    pub fn opens(&mut self, opens: Opens<P>) {
        self.opens.push(opens);
    }

    pub fn closing(&mut self, closing: Closing<P>) {
        self.closing.push(closing);
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
