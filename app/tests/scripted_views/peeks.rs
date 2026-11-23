use nova_files_thumbnails_api::Peeked;

use super::*;

const TRIP: u32 = 0;

fn folders_listed(cx: &mut TestAppContext) -> (&mut VisualTestContext, Core, ListingId) {
    cx.update(nova_files_window::bind_window_keys);
    let (_, cx, mut core) = window(cx);
    let (id, path) = core.next_listing().unwrap();
    let entries: Arc<[Entry]> = ["trip", "notes.txt"]
        .into_iter()
        .zip(0..)
        .map(|(name, index)| Entry {
            id: EntryId(index),
            name: name.into(),
            kind: if index == TRIP {
                EntryKind::Folder
            } else {
                EntryKind::File
            },
        })
        .collect();
    core.send(Event::FolderListed(Listing {
        id,
        path,
        entries,
        folders: None,
    }));
    cx.run_until_parked();
    (cx, core, id)
}

fn peeks_asked(core: &mut Core) -> Vec<bool> {
    core.commands()
        .into_iter()
        .filter_map(|command| match command {
            Command::Rows { peeks, count, .. } if count > 0 => Some(peeks),
            _ => None,
        })
        .collect()
}

fn size_asked(core: &mut Core) -> ThumbnailSize {
    core.commands()
        .into_iter()
        .filter_map(|command| match command {
            Command::Rows { size, count, .. } if count > 0 => Some(size),
            _ => None,
        })
        .next_back()
        .unwrap()
}

fn peek(id: ListingId, size: ThumbnailSize) -> ThumbnailsEvent {
    let peeked = |name: &str, png: Option<&[u8]>| Peeked {
        name: name.into(),
        kind: EntryKind::File,
        png: png.map(Into::into),
    };
    ThumbnailsEvent::Peek {
        id,
        entry: EntryId(TRIP),
        size,
        peeked: vec![
            peeked("photo.png", Some(PNG)),
            peeked("notes.txt", None),
            peeked(".hidden", None),
        ],
    }
}

fn shown(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    cx.run_until_parked();
    cx.debug_bounds(selector).is_some()
}

#[gpui::test]
fn only_views_that_draw_folders_ask_for_peeks(cx: &mut TestAppContext) {
    let (cx, mut core, _) = folders_listed(cx);
    assert_eq!(peeks_asked(&mut core), [false]);

    cx.simulate_keystrokes("ctrl-2");
    cx.run_until_parked();
    assert_eq!(peeks_asked(&mut core), [true]);

    cx.simulate_keystrokes("ctrl-1");
    cx.run_until_parked();
    assert_eq!(peeks_asked(&mut core), [false]);
}

#[gpui::test]
fn a_folder_in_icons_shows_what_it_holds_without_hidden_entries(cx: &mut TestAppContext) {
    let (cx, mut core, id) = folders_listed(cx);
    cx.simulate_keystrokes("ctrl-2");
    cx.run_until_parked();
    let size = size_asked(&mut core);

    core.send(peek(id, size));

    assert!(shown(cx, "peek-0"));
    assert!(shown(cx, "peek-0-0"));
    assert!(shown(cx, "peek-0-1"));
    assert!(!shown(cx, "peek-0-2"));
    assert!(!shown(cx, "peek-1"));
}

#[gpui::test]
fn the_smallest_icons_mark_folders_and_details_draws_them_plain(cx: &mut TestAppContext) {
    let (cx, mut core, id) = folders_listed(cx);
    cx.simulate_keystrokes("ctrl-2 ctrl-- ctrl--");
    cx.run_until_parked();
    let size = size_asked(&mut core);
    core.send(peek(id, size));
    assert!(shown(cx, "folder-mark-0"));
    assert!(!shown(cx, "peek-0"));

    cx.simulate_keystrokes("ctrl-1");
    assert!(!shown(cx, "folder-mark-0"));
    assert!(!shown(cx, "peek-0"));
}
