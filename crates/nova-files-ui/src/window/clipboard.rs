//! Copy, cut and paste. Copy and cut put the files selected on the
//! clipboard, as other file managers read them (see `FileClipboard`);
//! paste copies or moves them into the folder on screen as one of the
//! core's jobs (see `jobs`).
//!
//! GPUI's clipboard carries text only, so the files go as their paths in
//! plain text, with GNOME's list as the text's metadata, which a paste in
//! any Nova Files window reads back whole, names that aren't UTF-8 too.
//! Other apps get the paths as text, and a paste from them counts the
//! text only in the forms `FileClipboard` accepts.

use std::path::PathBuf;

use gpui::{ClipboardEntry, ClipboardItem, Context};
use nova_files_api::{ClipboardAction, FileClipboard, GNOME_COPIED_FILES, Offer, PLAIN_TEXT_UTF8};

use super::MainWindow;
use crate::island;

impl MainWindow {
    /// Puts the files selected on the clipboard, to be moved when `cut`.
    pub(crate) fn copy_selected(&mut self, cut: bool, cx: &mut Context<Self>) {
        // What is in the trash only goes back where it was.
        if self.pick.is_some() || self.in_trash() {
            return;
        }
        let paths = self.selected_paths();
        let action = if cut {
            ClipboardAction::Cut
        } else {
            ClipboardAction::Copy
        };
        let files = FileClipboard {
            action,
            paths: paths.clone(),
        };
        // Nothing selected offers nothing.
        let Some(item) = clipboard_item(&files) else {
            return;
        };
        cx.write_to_clipboard(item);
        self.show_note(island::clipboard_note(cut, &paths), None, cx);
    }

    /// Copies or moves the files on the clipboard into the folder on
    /// screen. Files cut from this folder stay where they are; files cut
    /// elsewhere leave the clipboard once pasted.
    pub(crate) fn paste(&mut self, cx: &Context<Self>) {
        if self.pick.is_some() || self.in_trash() {
            return;
        }
        let Some(folder) = self.folder().map(PathBuf::from) else {
            return;
        };
        let Some(files) = cx.read_from_clipboard().as_ref().and_then(files_in) else {
            return;
        };
        let cut = files.action == ClipboardAction::Cut;
        if let Some(operation) = files.paste_into(folder)
            && self.start_job(operation)
            && cut
        {
            // Cut files paste once: they are no longer where it says.
            cx.write_to_clipboard(ClipboardItem::new_string(String::new()));
        }
    }
}

/// The clipboard item for `files`: their paths as text, GNOME's list as
/// its metadata. `None` when there is no path to offer.
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

/// The files `item` holds, if it holds any.
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
            // Only macOS and Windows hand over files as paths.
            ClipboardEntry::ExternalPaths(_) | ClipboardEntry::Image(_) => {}
        }
    }
    FileClipboard::from_offers(&offers)
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
    fn nothing_selected_offers_nothing() {
        let files = FileClipboard {
            action: ClipboardAction::Copy,
            paths: Vec::new(),
        };
        assert_eq!(clipboard_item(&files), None);
    }
}
