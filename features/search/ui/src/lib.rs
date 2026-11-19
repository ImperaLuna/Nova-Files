#![forbid(unsafe_code)]

mod draw;

use std::path::Path;
use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, KeyDownEvent, Subscription, Window,
};
use nova_files_api::ListingId;
use nova_files_messages::Envelope;
use nova_files_search_api::{Command, Event};
use nova_files_shell::actions::FocusSearch;
use nova_files_shell::text::{TextField, TextFieldEvent};
use nova_files_shell::{
    BarRank, ChipRank, Entries, Host, LaidOver, Location, NavEvent, Navigator, OverLeft, Part,
    PerTab, Scene, Slots, TabId,
};

const PLACEHOLDER: &str = "Search";
const SEARCH_KEY: &str = "/";
const FIRST_TAB: TabId = TabId(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    Everywhere,
    HereOnly,
}

struct TabSearch {
    field: Entity<TextField>,
    scope: Scope,
    _changes: Subscription,
}

pub struct SearchPart {
    host: Host,
    tabs: PerTab<TabSearch>,
    _watching: Vec<Subscription>,
}

impl SearchPart {
    fn field(&mut self, cx: &mut Context<Self>) -> Entity<TextField> {
        self.here(cx).field.clone()
    }

    fn shown_field(&self, cx: &App) -> Option<Entity<TextField>> {
        self.tabs
            .get(self.tab(cx))
            .map(|search| search.field.clone())
    }

    pub fn query(&self, cx: &App) -> String {
        self.tabs
            .get(self.tab(cx))
            .map(|search| search.field.read(cx).text().to_owned())
            .unwrap_or_default()
    }

    pub fn scope(&self, cx: &App) -> Scope {
        self.tabs
            .get(self.tab(cx))
            .map(|search| search.scope)
            .unwrap_or_default()
    }

    pub fn set_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
        let search = self.here(cx);
        if search.scope == scope {
            return;
        }
        search.scope = scope;
        if self.host.entries.read(cx).searching() {
            self.find(cx);
        }
        cx.notify();
    }

    fn tab(&self, cx: &App) -> TabId {
        self.host.nav.read(cx).tab().unwrap_or(FIRST_TAB)
    }

    fn here(&mut self, cx: &mut Context<Self>) -> &mut TabSearch {
        let tab = self.tab(cx);
        self.tabs.or_insert_with(tab, || new_search(cx))
    }

    fn query_changed(&mut self, cx: &mut Context<Self>) {
        if self.query(cx).trim().is_empty() {
            self.end_search(cx);
        } else {
            self.find(cx);
        }
        cx.notify();
    }

    fn find(&self, cx: &mut Context<Self>) {
        let marks = self.query(cx);
        let query = marks.trim().to_owned();
        let scope = self.scope(cx);
        let ask = Rc::new(move |id: ListingId, from: &Location| {
            let within = (scope == Scope::HereOnly)
                .then(|| from.folder().map(Path::to_path_buf))
                .flatten();
            Envelope::new(Command::Find {
                id,
                query: query.clone(),
                within,
            })
        });
        self.host.entries.update(cx, |entries, cx| {
            entries.lay_over(LaidOver { ask, marks }, cx);
        });
    }

    fn end_search(&mut self, cx: &mut Context<Self>) {
        self.clear(cx);
        self.host.entries.update(cx, Entries::take_back);
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        let field = self.field(cx);
        field.update(cx, |field, cx| field.set_text("", cx));
    }

    fn focus_box(&mut self, cx: &mut Context<Self>) {
        let field = self.field(cx);
        field.update(cx, |field, cx| {
            let all = 0..field.text().len();
            field.select_range(all, cx);
        });
        let handle = field.focus_handle(cx);
        self.host.nav.update(cx, |nav, cx| nav.focus(handle, cx));
    }

    pub(crate) fn focus_results(&self, cx: &mut Context<Self>) {
        self.host.nav.update(cx, Navigator::focus_results);
    }

    fn field_event(&mut self, event: TextFieldEvent, cx: &mut Context<Self>) {
        match event {
            TextFieldEvent::Changed => self.query_changed(cx),
            TextFieldEvent::Submitted => self.focus_results(cx),
            TextFieldEvent::Cancelled => {
                self.end_search(cx);
                self.focus_results(cx);
            }
        }
    }

    #[expect(
        clippy::needless_pass_by_value,
        reason = "the event slot hands each event over by value"
    )]
    fn arrived(&mut self, event: Event, cx: &mut Context<Self>) {
        if let Event::FindFailed { id, .. } = event {
            self.host
                .entries
                .update(cx, |entries, cx| entries.listing_failed(id, cx));
        }
    }

    fn focus_search(&mut self, _: &FocusSearch, _: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.host.entries.read(cx).picker() {
            return false;
        }
        self.focus_box(cx);
        true
    }

    fn key_down(&mut self, event: &KeyDownEvent, scene: &Scene, cx: &mut Context<Self>) -> bool {
        let held = event.keystroke.modifiers;
        let plain = !(held.control || held.alt || held.platform || held.function);
        let slash = event.keystroke.key_char.as_deref() == Some(SEARCH_KEY);
        if !scene.picker && plain && slash {
            self.focus_box(cx);
            return true;
        }
        false
    }
}

fn new_search(cx: &mut Context<SearchPart>) -> TabSearch {
    let field = cx.new(|cx| TextField::new(cx).with_placeholder(PLACEHOLDER));
    let changes = cx.subscribe(&field, |part, _, event, cx| part.field_event(*event, cx));
    TabSearch {
        field,
        scope: Scope::default(),
        _changes: changes,
    }
}

impl Part for SearchPart {
    fn open(host: &Host, cx: &mut Context<Self>) -> Self {
        let watching = vec![
            cx.subscribe(&host.entries, |part, _, _: &OverLeft, cx| part.clear(cx)),
            cx.subscribe(&host.nav, |part, _, event: &NavEvent, cx| match event {
                NavEvent::TabShown(tab) => {
                    part.tabs.or_insert_with(*tab, || new_search(cx));
                }
                NavEvent::TabClosed(tab) => {
                    part.tabs.closed(*tab);
                }
                NavEvent::Asked | NavEvent::Left => {}
            }),
        ];
        let mut tabs = PerTab::default();
        tabs.set(FIRST_TAB, new_search(cx));
        Self {
            host: host.clone(),
            tabs,
            _watching: watching,
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.on_event(Self::arrived);
        slots.sends::<Command>();
        slots.before(Self::focus_search);
        slots.key_down(Self::key_down);
        slots.shrinking_bar_at(BarRank::FIRST, |part, _, _, cx| Some(part.bar(cx)));
        slots.path_chip_at(ChipRank::FIRST, Self::scope_chip);
        slots.overlay(Self::no_matches);
        slots.overlay(Self::match_count);
    }
}
