use std::path::{Path, PathBuf};

use gpui::{App, Context, WeakEntity};
use nova_files_api::{Command, Event, Listing, ListingId, Order};
use nova_files_shell::assets::Icon;
use nova_files_shell::{
    Host, KnownFolders, Line, Location, Part, Published, RowItem, Scene, SidebarAt, SidebarRow,
    Slots, TabId,
};

use crate::folder_tree::{FolderTree, Toggle, TreeRow};
use crate::shown::HOME_LABEL;

const ROW_PREFIX: &str = "tree-";
const TOGGLE_LABEL: &str = "Show or hide what is inside";

pub struct TreePart {
    host: Host,
    this: WeakEntity<Self>,
    tree: FolderTree,
    line: Line,
    asked: Option<(ListingId, PathBuf)>,
    next_listing: u64,
    shown: Option<(Option<TabId>, PathBuf)>,
}

impl TreePart {
    pub fn toggle(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.tree.toggle(path);
        self.list_next();
        cx.notify();
    }

    pub const fn settled(&self) -> bool {
        self.asked.is_none()
    }

    fn listed(&mut self, listing: &Listing, _: bool, cx: &mut Context<Self>) {
        let scene = self.host.scene(cx);
        if !scene.in_folder(&listing.path) {
            return;
        }
        let tab = self.host.nav.read(cx).tab();
        let shown = Some((tab, listing.path.clone()));
        let arrived = self.shown != shown;
        self.shown = shown;
        self.tree.listed(&listing.path, &listing.entries);
        if arrived {
            self.tree.reveal(&listing.path);
        }
        self.list_next();
        cx.notify();
    }

    fn heard(&mut self, event: Event, cx: &mut Context<Self>) {
        let asked = self.asked.as_ref().map(|(id, _)| *id);
        match event {
            Event::FolderListed(listing) if asked == Some(listing.id) => {
                self.asked = None;
                self.tree.listed(&listing.path, &listing.entries);
            }
            Event::FolderUnreadable { id, path, .. } if asked == Some(id) => {
                self.asked = None;
                self.tree.unreadable(&path);
            }
            _ => return,
        }
        self.list_next();
        cx.notify();
    }

    fn list_next(&mut self) {
        if self.asked.is_some() {
            return;
        }
        let Some(path) = self.tree.next_to_list() else {
            return;
        };
        let id = ListingId(self.next_listing);
        self.next_listing += 1;
        let sent = self.line.send(Command::ListFolder {
            id,
            path: path.clone(),
            order: Order::default(),
        });
        if sent.is_ok() {
            self.asked = Some((id, path));
        }
    }

    fn toggler(&self, path: PathBuf) -> RowItem {
        let part = self.this.clone();
        RowItem::new(Icon::Chevron, TOGGLE_LABEL, move |cx| {
            part.update(cx, |part, cx| part.toggle(&path, cx)).ok();
        })
    }
}

fn rows(part: &TreePart, scene: &Scene, cx: &App) -> Vec<SidebarRow> {
    let known = Published::<KnownFolders>::get(cx);
    part.tree
        .rows()
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let TreeRow {
                path,
                name,
                depth,
                toggle,
            } = row;
            let (glyph, name) = if depth == 0 {
                (Icon::Home, HOME_LABEL.into())
            } else {
                (
                    known.map_or(Icon::Folder, |known| known.icon_at(&path)),
                    name,
                )
            };
            let here = scene.in_folder(&path);
            let shown = SidebarRow::new(
                format!("{ROW_PREFIX}{index}"),
                glyph,
                name,
                Location::Folder(path.clone()),
            )
            .rank(SidebarRow::OWN)
            .depth(u8::try_from(depth).unwrap_or(u8::MAX))
            .here(here);
            match toggle {
                Toggle::None => shown,
                Toggle::Closed | Toggle::Open => {
                    shown.expander(toggle == Toggle::Open, part.toggler(path))
                }
            }
        })
        .collect()
}

impl Part for TreePart {
    fn open(host: &Host, cx: &mut Context<Self>) -> Self {
        let line = host.lines.open(cx, |part, envelope, cx| {
            if let Some(event) = envelope.taken_as::<Event>() {
                part.heard(event, cx);
            }
        });
        Self {
            host: host.clone(),
            this: cx.entity().downgrade(),
            tree: FolderTree::new(host.home.clone()),
            line,
            asked: None,
            next_listing: 0,
            shown: None,
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.listed(Self::listed);
        slots.sidebar(SidebarAt::Tree, rows);
    }
}
