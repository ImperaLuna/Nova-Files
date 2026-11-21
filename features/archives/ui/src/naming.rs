use std::ffi::{OsStr, OsString};

use nova_files_api::numbered::{Naming, first_free, stem};
use nova_files_api::{Entry, EntryKind, Packing};
use nova_files_archives_api::{ENDINGS, SEVEN_ENDINGS};

pub const fn extension(packing: Packing) -> &'static str {
    match packing {
        Packing::ZipDeflate | Packing::ZipZstd => ".zip",
        Packing::TarZstd => ".tar.zst",
        Packing::TarGzip => ".tar.gz",
        Packing::TarXz => ".tar.xz",
        Packing::SevenZip => ".7z",
    }
}

pub fn opens(entry: &Entry, seven: bool) -> bool {
    let name = entry.name.to_string_lossy().to_lowercase();
    let ending = ENDINGS
        .iter()
        .find(|ending| name.len() > ending.len() && name.ends_with(*ending));
    entry.kind == EntryKind::File
        && ending.is_some_and(|ending| seven || !SEVEN_ENDINGS.contains(ending))
}

pub fn archive_name(
    first: &Entry,
    packing: Packing,
    taken: impl Fn(&OsStr) -> bool,
) -> Option<OsString> {
    let base = match first.kind {
        EntryKind::Folder => first.name.as_os_str(),
        EntryKind::File => stem(&first.name),
    };
    let mut name = base.to_os_string();
    name.push(extension(packing));
    first_free(&name, Naming::File, taken)
}

#[cfg(test)]
mod tests {
    use nova_files_api::EntryId;

    use super::*;

    fn entry(name: &str, kind: EntryKind) -> Entry {
        Entry {
            id: EntryId(0),
            name: name.into(),
            kind,
        }
    }

    #[test]
    fn an_archive_is_named_after_the_first_entry() {
        let free = |_: &OsStr| false;
        let folder = entry("photos.2024", EntryKind::Folder);
        let file = entry("report.pdf", EntryKind::File);
        let bare = entry("notes", EntryKind::File);
        let tarball = entry("backup.tar.gz", EntryKind::File);
        assert_eq!(
            archive_name(&folder, Packing::ZipDeflate, free),
            Some("photos.2024.zip".into())
        );
        assert_eq!(
            archive_name(&file, Packing::SevenZip, free),
            Some("report.7z".into())
        );
        assert_eq!(
            archive_name(&bare, Packing::TarZstd, free),
            Some("notes.tar.zst".into())
        );
        assert_eq!(
            archive_name(&tarball, Packing::ZipDeflate, free),
            Some("backup.zip".into())
        );
    }

    #[test]
    fn a_taken_name_gets_the_number_the_core_gives() {
        let taken = |name: &OsStr| name == OsStr::new("docs.zip");
        let folder = entry("docs", EntryKind::Folder);
        assert_eq!(
            archive_name(&folder, Packing::ZipDeflate, taken),
            Some("docs (2).zip".into())
        );
    }

    fn file_opens(name: &str, seven: bool) -> bool {
        opens(&entry(name, EntryKind::File), seven)
    }

    #[test]
    fn seven_zip_formats_open_only_with_seven_zip() {
        for name in [
            "a.zip",
            "a.tar.gz",
            "a.tar.zst",
            "a.tar.xz",
            "a.tar",
            "a.TXZ",
        ] {
            assert!(file_opens(name, false), "{name}");
        }
        assert!(!file_opens("a.7z", false));
        assert!(!file_opens("a.RAR", false));
        assert!(file_opens("a.7z", true));
        assert!(file_opens("a.rar", true));
    }

    #[test]
    fn only_archive_files_open() {
        for name in ["a.txt", "syslog.2.gz", "dump.sql.xz", "trace.zst", ".zip"] {
            assert!(!file_opens(name, true), "{name}");
        }
        assert!(!opens(&entry("site.zip", EntryKind::Folder), true));
    }
}
