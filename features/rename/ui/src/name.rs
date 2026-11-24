use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};

use nova_files_api::Listing;

pub fn renamed(original: &OsStr, typed: &str) -> OsString {
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

pub fn free_name(listing: &Listing, base: &str) -> OsString {
    let taken = |name: &str| {
        listing
            .entries
            .iter()
            .any(|entry| entry.name == OsStr::new(name))
    };
    (1..=listing.entries.len() + 1)
        .map(|number| {
            if number == 1 {
                base.to_owned()
            } else {
                format!("{base} {number}")
            }
        })
        .find(|name| !taken(name))
        .map(OsString::from)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use nova_files_api::{Entry, EntryId, EntryKind, ListingId};

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
        let free = |names: &[&str]| free_name(&listing(names), "New folder");
        assert_eq!(free(&[]), "New folder");
        assert_eq!(free(&["New folder"]), "New folder 2");
        assert_eq!(
            free(&["New folder", "New folder 2", "New folder 4"]),
            "New folder 3"
        );
        assert_eq!(free_name(&listing(&["New folder"]), "New file"), "New file");
    }
}
