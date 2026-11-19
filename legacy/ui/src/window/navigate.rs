use std::path::{Component, Path, PathBuf};

use gpui::Context;
use nova_files_api::{Command, EntryId, EntryKind, Envelope, ListingId, Message, Order, SortKey};

use super::{MainWindow, Went};
use crate::location::Location;

impl MainWindow {
    pub fn open(&mut self, folder: &Path) {
        self.went_elsewhere();
        self.history.leave();
        self.request(folder);
    }

    pub(super) const fn went_elsewhere(&mut self) {
        self.went = Went::Elsewhere;
    }

    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        if let Some(place) = self.history.back() {
            self.went_elsewhere();
            self.go_to(&place, cx);
        }
    }

    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        if let Some(place) = self.history.forward() {
            self.went_elsewhere();
            self.go_to(&place, cx);
        }
    }

    fn go_to(&mut self, place: &Location, cx: &mut Context<Self>) {
        match place {
            Location::Start => self.arrive_at_start(cx),
            Location::Folder(folder) => {
                self.request(folder);
            }
            Location::Page(page) if self.parts.lists(page) => {
                self.request_listed(page.clone());
            }
            Location::Page(page) => self.arrive_at_page(page.clone(), cx),
        }
    }

    pub(super) fn request_place(&mut self, place: &Location) -> bool {
        match place {
            Location::Start => false,
            Location::Page(page) => self.request_listed(page.clone()),
            Location::Folder(folder) => self.request(folder),
        }
    }

    pub(super) fn replace_with(&mut self, place: &Location, cx: &mut Context<Self>) {
        self.went_elsewhere();
        self.history.replace_with(place.clone());
        self.go_to(place, cx);
        cx.notify();
    }

    pub fn back_target(&self) -> Option<&Location> {
        self.history.back_target()
    }

    pub fn forward_target(&self) -> Option<&Location> {
        self.history.forward_target()
    }

    pub(super) fn request(&mut self, folder: &Path) -> bool {
        let path = tidy(folder);
        let place = Location::Folder(path.clone());
        self.ask_listing(place, |id, order| Command::ListFolder { id, path, order })
    }

    pub(super) fn ask_listing<M: Message>(
        &mut self,
        place: Location,
        command: impl FnOnce(ListingId, Order) -> M,
    ) -> bool {
        let id = ListingId(self.next_listing);
        let command = Envelope::new(command(id, self.order));
        self.send_listing(place, id, Some(command))
    }

    pub(super) fn send_listing(
        &mut self,
        place: Location,
        id: ListingId,
        command: Option<Envelope>,
    ) -> bool {
        self.falling_back = false;
        self.next_listing += 1;
        let order = self.order;
        let sent = command
            .zip(self.connection.as_ref())
            .map(|(command, connection)| connection.send_envelope(command));
        let sent = matches!(sent, Some(Ok(())));
        if sent {
            self.search.folder_asked();
            self.asks_sent += 1;
            self.asked = Some(id);
            self.asked_order = order;
            self.requested = Some(place);
        }
        sent
    }

    pub const fn shown_order(&self) -> Order {
        self.shown_order
    }

    pub fn sort_by(&mut self, key: SortKey) {
        if self.searching() {
            return;
        }
        let Some(place) = self.place_asked_last() else {
            return;
        };
        let before = self.order;
        self.order = next_order(before, key);
        if !self.request_place(&place) {
            self.order = before;
        }
    }

    pub fn open_entry(&mut self, id: EntryId) {
        if let Some(folder) = self.folder_of_entry(id) {
            self.open(&folder);
        }
    }

    pub(super) fn folder_of_entry(&self, id: EntryId) -> Option<PathBuf> {
        let listing = self.listing.as_ref()?;
        let entry = usize::try_from(id.0)
            .ok()
            .and_then(|index| listing.entries.get(index))?;
        (entry.kind == EntryKind::Folder).then(|| listing.path_of(entry))
    }

    pub fn parent(&self) -> Option<PathBuf> {
        if let Some(Location::Page(page)) = self.location() {
            return page.parent();
        }
        self.folder()?.parent().map(Path::to_path_buf)
    }

    pub fn go_up(&mut self) {
        if let Some(parent) = self.parent() {
            self.open(&parent);
        }
    }
}

fn next_order(current: Order, key: SortKey) -> Order {
    let descending = if current.key == key {
        !current.descending
    } else {
        key != SortKey::Name
    };
    Order { key, descending }
}

pub fn tidy(path: &Path) -> PathBuf {
    let mut tidy = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => match tidy.components().next_back() {
                Some(Component::Normal(_)) => {
                    tidy.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                Some(Component::ParentDir | Component::CurDir) | None => tidy.push(part),
            },
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => tidy.push(part),
        }
    }
    if tidy.as_os_str().is_empty() {
        PathBuf::from(CURRENT_FOLDER)
    } else {
        tidy
    }
}

const CURRENT_FOLDER: &str = ".";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_on_the_column_in_use_turns_it_around() {
        let name = Order::default();
        let reversed = next_order(name, SortKey::Name);
        assert_eq!(
            reversed,
            Order {
                key: SortKey::Name,
                descending: true
            }
        );
        assert_eq!(next_order(reversed, SortKey::Name), name);
    }

    #[test]
    fn a_new_column_starts_a_to_z_by_name_and_largest_or_newest_first_otherwise() {
        let by_size = next_order(Order::default(), SortKey::Size);
        assert_eq!(
            by_size,
            Order {
                key: SortKey::Size,
                descending: true
            }
        );
        let by_modified = next_order(by_size, SortKey::Modified);
        assert!(by_modified.descending);
        assert_eq!(next_order(by_modified, SortKey::Name), Order::default());
    }

    #[test]
    fn tidy_resolves_dots_by_text() {
        assert_eq!(tidy(Path::new("/home/me/..")), PathBuf::from("/home"));
        assert_eq!(tidy(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
        assert_eq!(tidy(Path::new("/..")), PathBuf::from("/"));
        assert_eq!(tidy(Path::new("/")), PathBuf::from("/"));
        assert_eq!(tidy(Path::new("..")), PathBuf::from(".."));
        assert_eq!(tidy(Path::new("../../child")), PathBuf::from("../../child"));
        assert_eq!(tidy(Path::new("a/../../b")), PathBuf::from("../b"));
        assert_eq!(tidy(Path::new("a/..")), PathBuf::from("."));
        assert_eq!(tidy(Path::new(".")), PathBuf::from("."));
    }
}
