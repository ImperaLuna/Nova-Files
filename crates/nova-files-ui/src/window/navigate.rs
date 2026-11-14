//! Going places: a new folder, Back, Forward, Up and into an entry. Each
//! asks the core for a listing with a new id; the window moves once that
//! id answers (see `events`). Paths are tidied by their text alone, as a
//! user reads them.

use std::path::{Component, Path, PathBuf};

use nova_files_api::{Command, EntryId, EntryKind, ListingId};

use super::MainWindow;

impl MainWindow {
    /// Goes to `folder`, a new place in the history: asks the core for it,
    /// and the window moves there once it answers.
    pub fn open(&mut self, folder: &Path) {
        self.history.leave();
        self.request(folder);
    }

    /// Goes back to the place before this one, if there is one.
    pub fn go_back(&mut self) {
        if let Some(place) = self.history.back() {
            self.request(&place);
        }
    }

    /// Goes forward again after Back, if there is somewhere to go.
    pub fn go_forward(&mut self) {
        if let Some(place) = self.history.forward() {
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
    /// window moves there once it answers.
    pub(super) fn request(&mut self, folder: &Path) {
        let id = ListingId(self.next_listing);
        self.next_listing += 1;
        let path = tidy(folder);
        let sent = self
            .connection
            .as_ref()
            .map(|connection| connection.send(Command::ListFolder { id, path }));
        if matches!(sent, Some(Ok(()))) {
            self.asked = Some(id);
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
