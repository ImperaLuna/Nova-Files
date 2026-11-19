#![forbid(unsafe_code)]

mod cards;

use std::path::{Path, PathBuf};

use gpui::{AnyElement, Context, Subscription, Window};
use nova_files_places_api::Known;
use nova_files_shell::{CardRank, Host, Location, Part, Published, Scene, Slots, StartPage};
use nova_files_start_api::{Command, Event, RecentFolder, RecentId};

const RECENT_CARDS: u32 = 5;

pub struct StartPart {
    host: Host,
    recent: Vec<RecentFolder>,
    asked: RecentId,
    _places: Subscription,
}

impl StartPart {
    pub fn recent_folders(&self) -> &[RecentFolder] {
        &self.recent
    }

    fn on_start(&self, cx: &Context<Self>) -> bool {
        self.host.entries.read(cx).location() == Some(&Location::start())
    }

    fn ask(&mut self, cx: &Context<Self>) {
        self.asked = next_ask(self.asked);
        let known = Published::<Known>::get(cx);
        let places = known.into_iter().flat_map(|known| &known.places);
        let pinned = known.into_iter().flat_map(|known| known.pinned.iter());
        let skip: Vec<PathBuf> = std::iter::once(self.host.home.clone())
            .chain(places.map(|place| place.path.clone()))
            .chain(pinned.map(|pinned| pinned.path.clone()))
            .collect();
        self.host
            .link
            .send(Command::RecentFolders {
                id: self.asked,
                count: RECENT_CARDS,
                skip,
            })
            .ok();
    }

    fn arrived(&mut self, event: Event, cx: &mut Context<Self>) {
        let Event::RecentFolders { id, folders } = event;
        if id == self.asked {
            self.recent = folders;
            cx.notify();
        }
    }

    fn used(&mut self, folder: &Path, _: &mut Context<Self>) {
        self.host
            .link
            .send(Command::FolderUsed(folder.to_owned()))
            .ok();
    }

    fn open(&self, folder: &Path, cx: &mut Context<Self>) {
        let place = Location::Folder(folder.to_owned());
        self.host.nav.update(cx, |nav, cx| nav.go(place, cx));
    }

    fn open_in_new_tab(&self, folder: &Path, cx: &mut Context<Self>) {
        let place = Location::Folder(folder.to_owned());
        self.host
            .nav
            .update(cx, |nav, cx| nav.open_in_new_tab(place, cx));
    }
}

impl Part for StartPart {
    fn open(host: &Host, cx: &mut Context<Self>) -> Self {
        let places = cx.observe_global::<Published<Known>>(|part, cx| {
            if part.on_start(cx) {
                part.ask(cx);
            }
        });
        Self {
            host: host.clone(),
            recent: Vec::new(),
            asked: RecentId(0),
            _places: places,
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.on_event(Self::arrived);
        slots.sends::<Command>();
        slots.used(Self::used);
        slots.page_shown::<StartPage>(|part, _, _, cx| part.ask(cx));
        slots.refused(|part, _, _, cx| part.ask(cx));
        slots.start_card_at(CardRank::RECENT, recent_cards);
    }
}

fn recent_cards(
    part: &mut StartPart,
    _: &Scene,
    _: &mut Window,
    cx: &mut Context<StartPart>,
) -> Option<AnyElement> {
    cards::recent(part, cx)
}

const fn next_ask(id: RecentId) -> RecentId {
    RecentId(id.0 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_ask_has_a_new_id() {
        assert_eq!(next_ask(RecentId(4)), RecentId(5));
    }
}
