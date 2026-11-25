use nova_files_api::IconKind;

use super::{MEDIA_SEPARATOR, MimeTypes};

const NAME_SEPARATOR: &str = "-";
const GENERIC_SUFFIX: &str = "-x-generic";
const TEXT_MEDIA_NAME: &str = "text";
const EXECUTABLE: &str = "application/x-executable";
const SCRIPT_ICON: &str = "text-x-script";

const MEDIA_KINDS: [(&str, IconKind); 4] = [
    ("image", IconKind::Picture),
    ("video", IconKind::Video),
    ("audio", IconKind::Audio),
    ("text", IconKind::Text),
];

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
    pub fn icon_names(&self, mime: &str) -> Vec<String> {
        let ancestry = self.ancestry(mime);
        let media = self
            .canonical(mime)
            .split_once(MEDIA_SEPARATOR)
            .map(|(media, _)| media);
        let (own_media, other_media): (Vec<&String>, Vec<&String>) = ancestry
            .iter()
            .partition(|kind_of| kind_of.split_once(MEDIA_SEPARATOR).map(|(of, _)| of) == media);
        let is_script =
            media == Some(TEXT_MEDIA_NAME) && ancestry.iter().any(|kind_of| kind_of == EXECUTABLE);
        let candidates = own_media
            .into_iter()
            .flat_map(|kind_of| self.type_icon_names(kind_of))
            .chain(is_script.then(|| SCRIPT_ICON.to_owned()))
            .chain(
                other_media
                    .into_iter()
                    .flat_map(|kind_of| self.type_icon_names(kind_of)),
            )
            .chain(media.map(|media| format!("{media}{GENERIC_SUFFIX}")));
        let mut names: Vec<String> = Vec::new();
        for name in candidates {
            if !name.is_empty() && !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }

    fn type_icon_names(&self, kind_of: &str) -> impl Iterator<Item = String> {
        let icon = self.icons.get(kind_of).cloned();
        let own = kind_of.replace(MEDIA_SEPARATOR, NAME_SEPARATOR);
        let generic = self.generic_icons.get(kind_of).cloned();
        icon.into_iter().chain([own]).chain(generic)
    }

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
