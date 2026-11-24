#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use gpui::{App, ClipboardEntry, ClipboardItem, Context, Window};
use nova_files_clipboard_api::{
    ClipboardAction, FileClipboard, GNOME_COPIED_FILES, Offer, PLAIN_TEXT_UTF8,
};
use nova_files_shell::assets::Icon;
use nova_files_shell::{
    Host, Location, MenuAt, MenuGroup, MenuItem, Part, Placed, Scene, Slots, command_spec,
};

mod actions {
    #![expect(
        clippy::derive_partial_eq_without_eq,
        reason = "GPUI's actions! macro derives PartialEq only"
    )]
    gpui::actions!(
        nova_files_clipboard,
        [CopySelected, CutSelected, Paste, CopyPath, CopyFolderPath]
    );
}

pub use actions::{CopyFolderPath, CopyPath, CopySelected, CutSelected, Paste};

command_spec!(
    COPY,
    CopySelected,
    "file.copy",
    "Copy",
    ["ctrl-c"],
    needs[TakeOut]
);
command_spec!(
    CUT,
    CutSelected,
    "file.cut",
    "Cut",
    ["ctrl-x"],
    needs[TakeOut, Change]
);
command_spec!(
    PASTE,
    Paste,
    "file.paste",
    "Paste",
    ["ctrl-v"],
    needs[Change]
);
command_spec!(
    COPY_PATH,
    CopyPath,
    "file.copy-path",
    "Copy path",
    ["ctrl-shift-c"],
    needs[TakeOut]
);
command_spec!(
    COPY_FOLDER_PATH,
    CopyFolderPath,
    "folder.copy-path",
    "Copy path",
    [],
    needs[TakeOut]
);

const ITEMS: &str = "items";
const COPIED_PATH: &str = "Copied path";
const COPIED_PATHS: &str = "paths";
const LINE: &str = "\n";
const ROOT: &str = "/";

pub struct ClipboardPart {
    host: Host,
}

impl ClipboardPart {
    fn copy(&mut self, _: &CopySelected, _: &mut Window, cx: &mut Context<Self>) {
        self.put(ClipboardAction::Copy, cx);
    }

    fn cut(&mut self, _: &CutSelected, _: &mut Window, cx: &mut Context<Self>) {
        self.put(ClipboardAction::Cut, cx);
    }

    fn put(&self, action: ClipboardAction, cx: &mut Context<Self>) {
        let paths = self.host.entries.read(cx).selected_paths();
        let note = note(action, &paths);
        let Some(item) = clipboard_item(&FileClipboard { action, paths }) else {
            return;
        };
        cx.write_to_clipboard(item);
        self.host
            .island
            .update(cx, |island, cx| island.note(note, None, cx));
    }

    fn copy_path(&mut self, _: &CopyPath, _: &mut Window, cx: &mut Context<Self>) {
        let selected = self.host.entries.read(cx).selected_paths();
        if selected.is_empty() {
            self.put_paths(&self.here(cx), cx);
        } else {
            self.put_paths(&selected, cx);
        }
    }

    fn copy_folder_path(&mut self, _: &CopyFolderPath, _: &mut Window, cx: &mut Context<Self>) {
        self.put_paths(&self.here(cx), cx);
    }

    fn here(&self, cx: &App) -> Vec<PathBuf> {
        let scene = self.host.scene(cx);
        let here = scene.location.as_ref().and_then(Location::folder);
        here.map(Path::to_path_buf).into_iter().collect()
    }

    fn put_paths(&self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let Some(note) = path_note(paths) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(path_text(paths)));
        self.host
            .island
            .update(cx, |island, cx| island.note(note, None, cx));
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let scene = self.host.scene(cx);
        let Some(folder) = scene.location.as_ref().and_then(Location::folder) else {
            return;
        };
        let Some(files) = cx.read_from_clipboard().as_ref().and_then(files_in) else {
            return;
        };
        let Some(operation) = files.paste_into(folder.to_path_buf()) else {
            return;
        };
        self.host
            .island
            .update(cx, |island, cx| island.start_job(operation, cx));
        if files.action == ClipboardAction::Cut {
            cx.write_to_clipboard(ClipboardItem::new_string(String::new()));
        }
    }
}

const fn not_picking(_: &ClipboardPart, scene: &Scene, _: &App) -> bool {
    !scene.picker
}

const fn pastes_here(part: &ClipboardPart, scene: &Scene, cx: &App) -> bool {
    not_picking(part, scene, cx) && !scene.searching
}

fn has_files(cx: &App) -> bool {
    cx.read_from_clipboard()
        .as_ref()
        .and_then(files_in)
        .is_some()
}

fn entry_items(_: &ClipboardPart, _: &Scene, _: &App) -> Vec<MenuItem> {
    vec![
        MenuItem::run(Icon::Cut, &CUT),
        MenuItem::run(Icon::Copy, &COPY),
        MenuItem::run(Icon::CopyPath, &COPY_PATH),
    ]
}

fn sheet_items(_: &ClipboardPart, scene: &Scene, cx: &App) -> Vec<MenuItem> {
    let in_folder = scene.location.as_ref().and_then(Location::folder).is_some();
    if scene.searching || !in_folder {
        return Vec::new();
    }
    let paste = has_files(cx).then(|| MenuItem::run(Icon::Paste, &PASTE));
    paste
        .into_iter()
        .chain([MenuItem::run(Icon::CopyPath, &COPY_FOLDER_PATH)])
        .collect()
}

fn island_buttons(_: &ClipboardPart, _: &Scene, _: &App) -> Vec<MenuItem> {
    vec![
        MenuItem::run(Icon::Copy, &COPY)
            .with_selector("island-copy")
            .icon_only(),
        MenuItem::run(Icon::Cut, &CUT)
            .with_selector("island-cut")
            .icon_only(),
    ]
}

fn note(action: ClipboardAction, paths: &[PathBuf]) -> String {
    let verb = match action {
        ClipboardAction::Copy => "Copied",
        ClipboardAction::Cut => "Cut",
    };
    format!("{verb} {}", what(paths))
}

fn path_note(paths: &[PathBuf]) -> Option<String> {
    match paths {
        [] => None,
        [_] => Some(COPIED_PATH.to_owned()),
        many => Some(format!("Copied {} {COPIED_PATHS}", many.len())),
    }
}

fn path_text(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.to_string_lossy())
        .collect::<Vec<_>>()
        .join(LINE)
}

fn what(paths: &[PathBuf]) -> String {
    match paths {
        [one] => name_of(one),
        many => format!("{} {ITEMS}", many.len()),
    }
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || ROOT.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn clipboard_item(files: &FileClipboard) -> Option<ClipboardItem> {
    let offers = files.offers();
    let bytes_of = |mime: &str| {
        offers
            .iter()
            .find(|offer| offer.mime == mime)
            .map(|offer| offer.bytes.clone())
    };
    let text = String::from_utf8_lossy(&bytes_of(PLAIN_TEXT_UTF8)?).into_owned();
    let gnome = String::from_utf8(bytes_of(GNOME_COPIED_FILES)?).ok()?;
    Some(ClipboardItem::new_string_with_metadata(text, gnome))
}

fn files_in(item: &ClipboardItem) -> Option<FileClipboard> {
    let mut offers = Vec::new();
    for entry in item.entries() {
        match entry {
            ClipboardEntry::String(text) => {
                if let Some(gnome) = &text.metadata {
                    offers.push(Offer::new(GNOME_COPIED_FILES, gnome.as_bytes()));
                }
                offers.push(Offer::new(PLAIN_TEXT_UTF8, text.text.as_bytes()));
            }
            ClipboardEntry::ExternalPaths(_) | ClipboardEntry::Image(_) => {}
        }
    }
    FileClipboard::from_offers(&offers)
}

impl Part for ClipboardPart {
    fn open(host: &Host, _: &mut Context<Self>) -> Self {
        Self { host: host.clone() }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.command(&COPY, not_picking, Self::copy);
        slots.command(&CUT, not_picking, Self::cut);
        slots.command(&PASTE, pastes_here, Self::paste);
        slots.command(&COPY_PATH, not_picking, Self::copy_path);
        slots.command(&COPY_FOLDER_PATH, not_picking, Self::copy_folder_path);
        slots.menu_in(MenuAt::Entry, MenuGroup::Edit, Placed::FIRST, entry_items);
        slots.menu_in(MenuAt::Sheet, MenuGroup::Edit, Placed::SECOND, sheet_items);
        slots.menu_in(
            MenuAt::Island,
            MenuGroup::Edit,
            Placed::FIRST,
            island_buttons,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;
    use std::path::Path;

    use super::*;

    #[test]
    fn files_go_through_the_clipboard_whole_and_keep_whether_they_were_cut() {
        let odd = Path::new("/tmp").join(OsStr::from_bytes(b"caf\xe9 notes"));
        for action in [ClipboardAction::Copy, ClipboardAction::Cut] {
            let files = FileClipboard {
                action,
                paths: vec![PathBuf::from("/tmp/a b.txt"), odd.clone()],
            };
            let item = clipboard_item(&files).unwrap();
            assert_eq!(files_in(&item), Some(files));
        }
    }

    #[test]
    fn other_apps_read_the_paths_as_text() {
        let files = FileClipboard {
            action: ClipboardAction::Copy,
            paths: vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")],
        };
        assert_eq!(
            clipboard_item(&files).unwrap().text().as_deref(),
            Some("/tmp/a\n/tmp/b")
        );
    }

    #[test]
    fn text_from_another_app_is_files_only_as_file_uris() {
        let uris = ClipboardItem::new_string("file:///tmp/a\nfile:///tmp/b".to_owned());
        assert_eq!(
            files_in(&uris),
            Some(FileClipboard {
                action: ClipboardAction::Copy,
                paths: vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")],
            })
        );
        let typed = ClipboardItem::new_string("/tmp/a".to_owned());
        assert_eq!(files_in(&typed), None);
    }

    #[test]
    fn a_lone_path_goes_by_its_name_and_the_root_by_a_slash() {
        let paths = |names: &[&str]| names.iter().map(PathBuf::from).collect::<Vec<_>>();
        assert_eq!(
            note(ClipboardAction::Copy, &paths(&["notes.txt"])),
            "Copied notes.txt"
        );
        assert_eq!(
            note(ClipboardAction::Cut, &paths(&["a", "b", "c"])),
            "Cut 3 items"
        );
        assert_eq!(name_of(Path::new("/")), "/");
    }

    #[test]
    fn paths_go_out_as_plain_lines_and_the_note_counts_them() {
        let paths = vec![PathBuf::from("/tmp/a b.txt"), PathBuf::from("/tmp/c")];
        assert_eq!(path_text(&paths), "/tmp/a b.txt\n/tmp/c");
        assert_eq!(path_note(&paths).as_deref(), Some("Copied 2 paths"));
        assert_eq!(path_note(&paths[..1]).as_deref(), Some("Copied path"));
        assert_eq!(path_note(&[]), None);
    }

    #[test]
    fn nothing_selected_offers_nothing() {
        let files = FileClipboard {
            action: ClipboardAction::Copy,
            paths: Vec::new(),
        };
        assert_eq!(clipboard_item(&files), None);
    }
}
