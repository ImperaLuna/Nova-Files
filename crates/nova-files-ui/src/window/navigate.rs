//! Going places: a new folder, Back, Forward, Up and into an entry. Each
//! asks the core for a listing with a new id, in the window's order; the
//! window moves once that id answers (see `events`). Changing the order
//! reads the folder again. Paths are tidied by their text alone, as a user
//! reads them.

use std::path::{Component, Path, PathBuf};

use nova_files_api::{Command, EntryId, EntryKind, ListingId, Order, SortKey};

use super::MainWindow;

impl MainWindow {
    /// Goes to `folder`, a new place in the history: asks the core for it,
    /// and the window moves there once it answers.
    pub fn open(&mut self, folder: &Path) {
        self.forget_drive_to_open();
        self.history.leave();
        self.request(folder);
    }

    /// Goes back to the place before this one, if there is one.
    pub fn go_back(&mut self) {
        if let Some(place) = self.history.back() {
            self.forget_drive_to_open();
            self.request(&place);
        }
    }

    /// Goes forward again after Back, if there is somewhere to go.
    pub fn go_forward(&mut self) {
        if let Some(place) = self.history.forward() {
            self.forget_drive_to_open();
            self.request(&place);
        }
    }

    /// Where Back goes, if anywhere.
    pub fn back_target(&self) -> Option<&Path> {
        self.history.back_target()
    }

    /// Where Forward goes, if anywhere.
    pub fn forward_target(&self) -> Option<&Path> {
        self.history.forward_target()
    }

    /// Asks the core for `folder`, leaving the history as it is: the
    /// window moves there once it answers. Whether the ask was sent.
    pub(super) fn request(&mut self, folder: &Path) -> bool {
        let id = ListingId(self.next_listing);
        self.next_listing += 1;
        let path = tidy(folder);
        let order = self.order;
        let sent = self.connection.as_ref().map(|connection| {
            connection.send(Command::ListFolder {
                id,
                path: path.clone(),
                order,
            })
        });
        let sent = matches!(sent, Some(Ok(())));
        if sent {
            self.asked = Some(id);
            self.asked_order = order;
            self.requested = Some(path);
        }
        sent
    }

    /// The order of the listing on screen. A new order shows once its
    /// listing arrives.
    pub const fn shown_order(&self) -> Order {
        self.shown_order
    }

    /// Orders the list by `key`: the other way round when it already is,
    /// otherwise A to Z by name and newest or largest first by the
    /// others. The folder asked for last (the one on screen, or the one
    /// still on its way) is read again in the new order; the window keeps
    /// its place in the history.
    pub fn sort_by(&mut self, key: SortKey) {
        let folder = if self.waiting() {
            self.requested.clone()
        } else {
            self.folder().map(Path::to_path_buf)
        };
        let Some(folder) = folder else { return };
        let before = self.order;
        self.order = next_order(before, key);
        if !self.request(&folder) {
            self.order = before;
        }
    }

    /// Goes into the entry `id` of the folder on screen, if it is a folder.
    pub fn open_entry(&mut self, id: EntryId) {
        let Some(listing) = &self.listing else { return };
        let entry = usize::try_from(id.0)
            .ok()
            .and_then(|index| listing.entries.get(index));
        if let Some(entry) = entry
            && entry.kind == EntryKind::Folder
        {
            let folder = listing.path.join(&entry.name);
            self.open(&folder);
        }
    }

    /// The folder Up goes to, if there is one.
    pub fn parent(&self) -> Option<PathBuf> {
        self.folder()?.parent().map(Path::to_path_buf)
    }

    /// Goes to the parent of the folder on screen.
    pub fn go_up(&mut self) {
        if let Some(parent) = self.parent() {
            self.open(&parent);
        }
    }
}

/// The order after a click on `key`'s column head, from `current`.
fn next_order(current: Order, key: SortKey) -> Order {
    let descending = if current.key == key {
        !current.descending
    } else {
        key != SortKey::Name
    };
    Order { key, descending }
}

/// `path` with `.` dropped and each `..` taking off the part before it,
/// worked out from the text alone. `..` at the root stays at the root.
pub fn tidy(path: &Path) -> PathBuf {
    let mut tidy = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => match tidy.components().next_back() {
                Some(Component::Normal(_)) => {
                    tidy.pop();
                }
                // `..` at the root stays at the root.
                Some(Component::RootDir | Component::Prefix(_)) => {}
                // Nothing left to take off: the `..` stays.
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

/// The relative path that names the folder it is relative to.
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
        // A relative path keeps the `..` it can't take off.
        assert_eq!(tidy(Path::new("..")), PathBuf::from(".."));
        assert_eq!(tidy(Path::new("../../child")), PathBuf::from("../../child"));
        assert_eq!(tidy(Path::new("a/../../b")), PathBuf::from("../b"));
        assert_eq!(tidy(Path::new("a/..")), PathBuf::from("."));
        assert_eq!(tidy(Path::new(".")), PathBuf::from("."));
    }
}
