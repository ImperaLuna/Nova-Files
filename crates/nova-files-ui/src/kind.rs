//! What kind of file an entry is, for the colored tag on its icon. The kind
//! comes from the name's extension alone, so it costs no disk read; a
//! name without a known one is `Other`.

use std::ffi::OsStr;
use std::path::Path;

use gpui::Rgba;

use crate::theme::Tags;

/// The kinds a file icon's tag tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Image,
    Document,
    Code,
    Archive,
    Media,
    Other,
}

/// Known extensions, lower case, and their kinds.
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

impl FileKind {
    /// The kind of a file called `name`.
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

    /// The color of this kind's tag.
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
