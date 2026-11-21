//! The name endings of archives, written down here and nowhere else.
//!
//! The icon, the name a new archive gets, the names that open as an
//! archive and the folder an archive unpacks into all read them from here.

use std::ffi::OsStr;

use crate::Packing;

const TAR_GZ: &str = ".tar.gz";
const TAR_XZ: &str = ".tar.xz";
const TAR_ZST: &str = ".tar.zst";
const TAR_BZ2: &str = ".tar.bz2";
const ZIP: &str = ".zip";
const SEVEN_ZIP: &str = ".7z";
const RAR: &str = ".rar";

/// The name endings, in lowercase, of the archives the core unpacks.
pub const ENDINGS: [&str; 13] = [
    TAR_GZ, TAR_XZ, TAR_ZST, TAR_BZ2, ".tgz", ".txz", ".tzst", ".tbz2", ".tbz", ".tar", ZIP,
    SEVEN_ZIP, RAR,
];

/// The endings in [`ENDINGS`] the core unpacks only with an installed
/// 7-Zip.
pub const SEVEN_ENDINGS: [&str; 2] = [SEVEN_ZIP, RAR];

/// The endings in [`ENDINGS`] made of two, which a name keeps whole when
/// it takes a number.
pub const TARBALLS: [&str; 4] = [TAR_GZ, TAR_XZ, TAR_ZST, TAR_BZ2];

/// The ending in [`ENDINGS`] that `name` has, in any case, when something
/// comes before it.
pub fn ending(name: &OsStr) -> Option<&'static str> {
    let name = name.as_encoded_bytes().to_ascii_lowercase();
    ENDINGS
        .into_iter()
        .find(|ending| name.len() > ending.len() && name.ends_with(ending.as_bytes()))
}

impl Packing {
    /// Every kind of archive a compress makes.
    pub const ALL: [Self; 6] = [
        Self::ZipDeflate,
        Self::ZipZstd,
        Self::TarZstd,
        Self::TarGzip,
        Self::TarXz,
        Self::SevenZip,
    ];

    /// The name ending an archive of this kind gets.
    pub const fn ending(self) -> &'static str {
        match self {
            Self::ZipDeflate | Self::ZipZstd => ZIP,
            Self::TarZstd => TAR_ZST,
            Self::TarGzip => TAR_GZ,
            Self::TarXz => TAR_XZ,
            Self::SevenZip => SEVEN_ZIP,
        }
    }

    /// Whether an archive of this kind can be locked with a password.
    pub const fn locks(self) -> bool {
        match self {
            Self::ZipDeflate | Self::ZipZstd | Self::SevenZip => true,
            Self::TarZstd | Self::TarGzip | Self::TarXz => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_longest_ending_wins() {
        assert_eq!(ending(OsStr::new("a.tar.gz")), Some(".tar.gz"));
        assert_eq!(ending(OsStr::new("a.TAR")), Some(".tar"));
        assert_eq!(ending(OsStr::new("a.TXZ")), Some(".txz"));
        assert_eq!(ending(OsStr::new(".zip")), None);
        assert_eq!(ending(OsStr::new("dump.sql.xz")), None);
    }

    #[test]
    fn only_zip_and_7z_take_a_password() {
        let locking: Vec<_> = Packing::ALL.into_iter().filter(|p| p.locks()).collect();
        assert_eq!(
            locking,
            [Packing::ZipDeflate, Packing::ZipZstd, Packing::SevenZip]
        );
    }

    #[test]
    fn every_kind_made_opens_again() {
        for packing in Packing::ALL {
            assert!(ENDINGS.contains(&packing.ending()), "{packing:?}");
        }
    }
}
