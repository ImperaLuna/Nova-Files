use std::ffi::OsStr;
use std::path::Path;

use gpui::Rgba;
use nova_files_api::{EntryKind, IconKind};

use crate::assets::Icon;
use crate::theme::{Colors, Tags};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Image,
    Document,
    Code,
    Archive,
    Media,
    Other,
}

const KNOWN: &[(&str, FileKind)] = &[
    ("jpg", FileKind::Image),
    ("jpeg", FileKind::Image),
    ("png", FileKind::Image),
    ("webp", FileKind::Image),
    ("gif", FileKind::Image),
    ("heic", FileKind::Image),
    ("avif", FileKind::Image),
    ("svg", FileKind::Image),
    ("pdf", FileKind::Document),
    ("odt", FileKind::Document),
    ("ods", FileKind::Document),
    ("odp", FileKind::Document),
    ("docx", FileKind::Document),
    ("xlsx", FileKind::Document),
    ("csv", FileKind::Document),
    ("epub", FileKind::Document),
    ("rs", FileKind::Code),
    ("c", FileKind::Code),
    ("h", FileKind::Code),
    ("cpp", FileKind::Code),
    ("py", FileKind::Code),
    ("js", FileKind::Code),
    ("ts", FileKind::Code),
    ("sh", FileKind::Code),
    ("nix", FileKind::Code),
    ("toml", FileKind::Code),
    ("json", FileKind::Code),
    ("qml", FileKind::Code),
    ("go", FileKind::Code),
    ("java", FileKind::Code),
    ("html", FileKind::Code),
    ("css", FileKind::Code),
    ("yaml", FileKind::Code),
    ("yml", FileKind::Code),
    ("lock", FileKind::Code),
    ("zip", FileKind::Archive),
    ("tar", FileKind::Archive),
    ("gz", FileKind::Archive),
    ("xz", FileKind::Archive),
    ("zst", FileKind::Archive),
    ("7z", FileKind::Archive),
    ("rar", FileKind::Archive),
    ("mp4", FileKind::Media),
    ("mkv", FileKind::Media),
    ("webm", FileKind::Media),
    ("mov", FileKind::Media),
    ("mp3", FileKind::Media),
    ("flac", FileKind::Media),
    ("ogg", FileKind::Media),
    ("opus", FileKind::Media),
    ("wav", FileKind::Media),
];

const AUDIO: [&str; 5] = ["mp3", "flac", "ogg", "opus", "wav"];

pub fn icon_kind(name: &OsStr, kind: EntryKind) -> IconKind {
    if kind == EntryKind::Folder {
        return IconKind::Folder;
    }
    match FileKind::of(name) {
        FileKind::Image => IconKind::Picture,
        FileKind::Document | FileKind::Code => IconKind::Text,
        FileKind::Archive => IconKind::Archive,
        FileKind::Media if is_audio(name) => IconKind::Audio,
        FileKind::Media => IconKind::Video,
        FileKind::Other => IconKind::File,
    }
}

pub const fn entry_kind(kind: EntryKind) -> IconKind {
    match kind {
        EntryKind::Folder => IconKind::Folder,
        EntryKind::File => IconKind::File,
    }
}

pub const fn kind_glyph(kind: EntryKind, colors: &Colors) -> (Icon, Rgba) {
    built_in(entry_kind(kind), colors)
}

pub const fn built_in(kind: IconKind, colors: &Colors) -> (Icon, Rgba) {
    let tint = match kind {
        IconKind::Folder => colors.folder,
        _ => colors.text_soft,
    };
    (glyph_of(kind), tint)
}

pub const fn glyph_of(kind: IconKind) -> Icon {
    match kind {
        IconKind::Folder => Icon::Folder,
        IconKind::File => Icon::File,
        IconKind::Text => Icon::FileText,
        IconKind::Picture => Icon::FilePicture,
        IconKind::Video => Icon::FileVideo,
        IconKind::Audio => Icon::FileAudio,
        IconKind::Archive => Icon::FileArchive,
        IconKind::Program => Icon::FileProgram,
    }
}

fn is_audio(name: &OsStr) -> bool {
    Path::new(name).extension().is_some_and(|extension| {
        let extension = extension.to_string_lossy().to_lowercase();
        AUDIO.contains(&extension.as_str())
    })
}

impl FileKind {
    pub fn of(name: &OsStr) -> Self {
        let Some(extension) = Path::new(name).extension() else {
            return Self::Other;
        };
        let extension = extension.to_string_lossy().to_lowercase();
        KNOWN
            .iter()
            .find(|(known, _)| *known == extension)
            .map_or(Self::Other, |(_, kind)| *kind)
    }

    pub const fn tag(self, tags: Tags) -> Rgba {
        match self {
            Self::Image => tags.image,
            Self::Document => tags.document,
            Self::Code => tags.code,
            Self::Archive => tags.archive,
            Self::Media => tags.media,
            Self::Other => tags.other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Colors;

    #[test]
    fn kind_comes_from_the_extension() {
        let kind = |name: &str| FileKind::of(OsStr::new(name));
        assert_eq!(kind("photo.JPG"), FileKind::Image);
        assert_eq!(kind("clip.mkv"), FileKind::Media);
        assert_eq!(kind("notes.pdf"), FileKind::Document);
        assert_eq!(kind("lib.rs"), FileKind::Code);
        assert_eq!(kind("backup.tar.zst"), FileKind::Archive);
        assert_eq!(kind("data.xyz"), FileKind::Other);
        assert_eq!(kind("Makefile"), FileKind::Other);
        assert_eq!(kind(".bashrc"), FileKind::Other);
    }

    #[test]
    fn the_built_in_icon_comes_from_the_name() {
        let file = |name: &str| icon_kind(OsStr::new(name), EntryKind::File);
        assert_eq!(file("a.PNG"), IconKind::Picture);
        assert_eq!(file("a.pdf"), IconKind::Text);
        assert_eq!(file("a.rs"), IconKind::Text);
        assert_eq!(file("a.zip"), IconKind::Archive);
        assert_eq!(file("a.FLAC"), IconKind::Audio);
        assert_eq!(file("a.mkv"), IconKind::Video);
        assert_eq!(file("Makefile"), IconKind::File);
        let folder = icon_kind(OsStr::new("a.png"), EntryKind::Folder);
        assert_eq!(folder, IconKind::Folder);
    }

    #[test]
    fn each_kind_takes_its_own_tag() {
        let tags = Colors::default().tags;
        assert_eq!(FileKind::Image.tag(tags), tags.image);
        assert_eq!(FileKind::Document.tag(tags), tags.document);
        assert_eq!(FileKind::Code.tag(tags), tags.code);
        assert_eq!(FileKind::Archive.tag(tags), tags.archive);
        assert_eq!(FileKind::Media.tag(tags), tags.media);
        assert_eq!(FileKind::Other.tag(tags), tags.other);
    }
}
