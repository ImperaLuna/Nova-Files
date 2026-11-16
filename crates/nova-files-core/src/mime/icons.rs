//! The icon names a MIME type is drawn with, and the kind of file it is
//! for the built-in icons.
//!
//! The names follow the shared MIME info spec, most specific first: the
//! type's own icon from `mime/icons`, its name with `/` made `-`
//! (`image-png`), the same for each type it is a kind of, then the generic
//! icon from `mime/generic-icons`, and last `<media>-x-generic`. An icon
//! theme is searched for each in turn. The kind comes from the type's
//! media (`image/*` is a picture), else from its generic icon.

use nova_files_api::IconKind;

use super::{MEDIA_SEPARATOR, MimeTypes};

const NAME_SEPARATOR: &str = "-";
const GENERIC_SUFFIX: &str = "-x-generic";

/// Media whose every type is one kind.
const MEDIA_KINDS: [(&str, IconKind); 4] = [
    ("image", IconKind::Picture),
    ("video", IconKind::Video),
    ("audio", IconKind::Audio),
    ("text", IconKind::Text),
];

/// Generic icons, from the shared MIME info database, and the kinds they
/// stand for.
const GENERIC_KINDS: [(&str, IconKind); 12] = [
    ("image-x-generic", IconKind::Picture),
    ("video-x-generic", IconKind::Video),
    ("audio-x-generic", IconKind::Audio),
    ("package-x-generic", IconKind::Archive),
    ("application-x-executable", IconKind::Program),
    ("text-x-generic", IconKind::Text),
    ("text-x-generic-template", IconKind::Text),
    ("text-x-script", IconKind::Text),
    ("text-html", IconKind::Text),
    ("x-office-document", IconKind::Text),
    ("x-office-spreadsheet", IconKind::Text),
    ("x-office-presentation", IconKind::Text),
];

impl MimeTypes {
    /// The icon names to look for, for a file of type `mime`, most
    /// specific first, each once.
    pub fn icon_names(&self, mime: &str) -> Vec<String> {
        let ancestry = self.ancestry(mime);
        let mut names: Vec<String> = Vec::new();
        let mut add = |name: String| {
            if !name.is_empty() && !names.contains(&name) {
                names.push(name);
            }
        };
        for kind_of in &ancestry {
            if let Some(icon) = self.icons.get(kind_of) {
                add(icon.clone());
            }
            add(kind_of.replace(MEDIA_SEPARATOR, NAME_SEPARATOR));
        }
        for kind_of in &ancestry {
            if let Some(icon) = self.generic_icons.get(kind_of) {
                add(icon.clone());
            }
        }
        if let Some((media, _)) = self.canonical(mime).split_once(MEDIA_SEPARATOR) {
            add(format!("{media}{GENERIC_SUFFIX}"));
        }
        names
    }

    /// The kind of file a `mime` is, for its built-in icon.
    pub fn icon_kind(&self, mime: &str) -> IconKind {
        let ancestry = self.ancestry(mime);
        let by_media = ancestry.iter().find_map(|kind_of| {
            let (media, _) = kind_of.split_once(MEDIA_SEPARATOR)?;
            MEDIA_KINDS
                .iter()
                .find(|(known, _)| *known == media)
                .map(|(_, kind)| *kind)
        });
        let by_generic = || {
            ancestry.iter().find_map(|kind_of| {
                let icon = self.generic_icons.get(kind_of)?;
                GENERIC_KINDS
                    .iter()
                    .find(|(known, _)| known == icon)
                    .map(|(_, kind)| *kind)
            })
        };
        by_media.or_else(by_generic).unwrap_or(IconKind::File)
    }
}
