use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::PathBuf;

use gpui::{App, AppContext as _, Context, Entity, Focusable as _, Subscription, Window};
use nova_files_api::{Entry, EntryKind, Listing, Operation};

use super::MainWindow;
use crate::selection::Selection;
use nova_files_shell::text::{TextField, TextFieldEvent};
use nova_files_shell::typed::stem;

const NEW_FOLDER: &str = "New folder";

pub struct Renaming {
    pub folder: PathBuf,
    pub name: OsString,
    pub field: Entity<TextField>,
    _events: Subscription,
    _blur: Subscription,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    pub folder: PathBuf,
    pub name: OsString,
    pub rename: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Waiting {
    #[default]
    Nothing,
    Entry(Wanted),
    Rename,
}

impl MainWindow {
    pub(crate) fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.renaming.is_some() || self.folder_rules().read_only || self.folder().is_none() {
            return;
        }
        let Some(listing) = &self.listing else { return };
        let paths = self.selected_paths();
        let ([path], None) = (&paths[..], &self.pick) else {
            return;
        };
        let Some(name) = path.file_name().map(OsStr::to_os_string) else {
            return;
        };
        let folder = listing.path.clone();
        let is_folder = listing
            .entries
            .iter()
            .any(|entry| entry.name == name && entry.kind == EntryKind::Folder);
        let text = name.to_string_lossy().into_owned();
        let field = cx.new(TextField::new);
        field.update(cx, |field, cx| {
            field.set_text(&text, cx);
            let selected = if is_folder {
                0..text.len()
            } else {
                stem(&text)
            };
            field.select_range(selected, cx);
        });
        let events = cx.subscribe_in(&field, window, |this, _, event, window, cx| match event {
            TextFieldEvent::Submitted => this.finish_rename(true, window, cx),
            TextFieldEvent::Cancelled => this.finish_rename(false, window, cx),
            TextFieldEvent::Changed => {}
        });
        let focus = field.read(cx).focus_handle(cx);
        let blur = cx.on_blur(&focus, window, |this, window, cx| {
            let drawn = this
                .rename_row()
                .is_some_and(|row| this.rows_drawn.contains(&row));
            this.finish_rename(drawn, window, cx);
        });
        window.focus(&focus, cx);
        self.renaming = Some(Renaming {
            folder,
            name,
            field,
            _events: events,
            _blur: blur,
        });
        cx.notify();
    }

    fn finish_rename(&mut self, keep: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(renaming) = self.renaming.take() else {
            return;
        };
        let typed = renaming.field.read(cx).text().to_owned();
        window.focus(&self.focus, cx);
        cx.notify();
        let name = renamed(&renaming.name, &typed);
        if !keep || typed.is_empty() || name == renaming.name {
            return;
        }
        self.waiting = Waiting::Entry(Wanted {
            folder: renaming.folder.clone(),
            name: name.clone(),
            rename: false,
        });
        self.start_job(Operation::Rename {
            path: renaming.folder.join(&renaming.name),
            name,
        });
    }

    fn rename_row(&self) -> Option<usize> {
        let renaming = self.renaming.as_ref()?;
        let listing = self.listing.as_ref()?;
        if listing.path != renaming.folder {
            return None;
        }
        listing
            .entries
            .iter()
            .position(|entry| entry.name == renaming.name)
    }

    pub fn rename_field(&self, index: usize) -> Option<&Entity<TextField>> {
        let field = &self.renaming.as_ref()?.field;
        (self.rename_row()? == index).then_some(field)
    }

    pub fn name_field(
        &self,
        (index, entry): (usize, &Entry),
        cx: &App,
    ) -> Option<Entity<TextField>> {
        self.rename_field(index)
            .cloned()
            .or_else(|| self.parts.name_field(entry, cx))
    }

    pub const fn renaming(&self) -> bool {
        self.renaming.is_some()
    }

    pub fn new_folder(&mut self) {
        let Some(listing) = &self.listing else { return };
        if self.picker()
            || self.renaming.is_some()
            || self.folder_rules().read_only
            || self.folder().is_none()
        {
            return;
        }
        let name = free_name(listing);
        let inside = listing.path.clone();
        self.waiting = Waiting::Entry(Wanted {
            folder: inside.clone(),
            name: name.clone(),
            rename: true,
        });
        self.start_job(Operation::NewFolder { inside, name });
    }

    pub(super) fn wanted_selection(&mut self, listing: &Listing) -> Option<Selection> {
        let Waiting::Entry(wanted) = &self.waiting else {
            return None;
        };
        if wanted.folder != listing.path {
            return None;
        }
        let selection = Selection::named(listing.id, &listing.entries, &wanted.name)?;
        self.waiting = if wanted.rename {
            Waiting::Rename
        } else {
            Waiting::Nothing
        };
        Some(selection)
    }

    pub(crate) fn start_wanted_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.waiting == Waiting::Rename {
            self.waiting = Waiting::Nothing;
            self.start_rename(window, cx);
        }
    }
}

fn renamed(original: &OsStr, typed: &str) -> OsString {
    let mut shown: Vec<(char, &[u8])> = Vec::new();
    for chunk in original.as_bytes().utf8_chunks() {
        let valid = chunk.valid();
        for (at, character) in valid.char_indices() {
            shown.push((character, &valid.as_bytes()[at..at + character.len_utf8()]));
        }
        if !chunk.invalid().is_empty() {
            shown.push((char::REPLACEMENT_CHARACTER, chunk.invalid()));
        }
    }
    let typed: Vec<char> = typed.chars().collect();
    let same = |(shown, typed): (&(char, &[u8]), &char)| shown.0 == *typed;
    let front = shown
        .iter()
        .zip(&typed)
        .take_while(|pair| same(*pair))
        .count();
    let room = shown.len().min(typed.len()) - front;
    let back = shown
        .iter()
        .rev()
        .zip(typed.iter().rev())
        .take(room)
        .take_while(|pair| same(*pair))
        .count();
    let mut bytes = Vec::new();
    for (_, kept) in &shown[..front] {
        bytes.extend_from_slice(kept);
    }
    let middle: String = typed[front..typed.len() - back].iter().collect();
    bytes.extend_from_slice(middle.as_bytes());
    for (_, kept) in &shown[shown.len() - back..] {
        bytes.extend_from_slice(kept);
    }
    OsString::from_vec(bytes)
}

fn free_name(listing: &Listing) -> OsString {
    let taken = |name: &str| {
        listing
            .entries
            .iter()
            .any(|entry| entry.name == OsStr::new(name))
    };
    (1..=listing.entries.len() + 1)
        .map(|number| {
            if number == 1 {
                NEW_FOLDER.to_owned()
            } else {
                format!("{NEW_FOLDER} {number}")
            }
        })
        .find(|name| !taken(name))
        .map(OsString::from)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nova_files_api::{Entry, EntryId, ListingId};

    use super::*;

    fn listing(names: &[&str]) -> Listing {
        Listing {
            id: ListingId(0),
            path: PathBuf::from("/a"),
            entries: names
                .iter()
                .zip(0..)
                .map(|(name, id)| Entry {
                    id: EntryId(id),
                    name: OsString::from(name),
                    kind: EntryKind::Folder,
                })
                .collect::<Arc<[Entry]>>(),
            folders: None,
        }
    }

    #[test]
    fn a_rename_keeps_the_bytes_the_user_left() {
        let odd = OsStr::from_bytes(b"caf\xe9 notes.txt");
        assert_eq!(
            renamed(odd, "caf\u{FFFD} notes.md"),
            OsStr::from_bytes(b"caf\xe9 notes.md")
        );
        assert_eq!(renamed(odd, "caf\u{FFFD} notes.txt"), odd);
        assert_eq!(renamed(odd, "plans.txt"), OsStr::new("plans.txt"));
        assert_eq!(renamed(odd, "cafe notes.txt"), OsStr::new("cafe notes.txt"));
        assert_eq!(
            renamed(OsStr::new("notes.txt"), "notes2.txt"),
            OsStr::new("notes2.txt")
        );
        assert_eq!(renamed(OsStr::new("aa"), "a"), OsStr::new("a"));
        assert_eq!(renamed(OsStr::new("a"), "aaa"), OsStr::new("aaa"));
    }

    #[test]
    fn a_new_folder_takes_the_first_free_name() {
        assert_eq!(free_name(&listing(&[])), "New folder");
        assert_eq!(free_name(&listing(&["New folder"])), "New folder 2");
        assert_eq!(
            free_name(&listing(&["New folder", "New folder 2", "New folder 4"])),
            "New folder 3"
        );
    }
}
