#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

use nova_files_api::archive::{SEVEN_ZIP, TAR_GZ, TAR_XZ, TAR_ZST, ZIP};
use nova_files_api::{Answer, Question};

/// What a window asks the archives core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Which formats can be made here. The answer is [`Event::Formats`],
    /// sent to the client that asked.
    Formats,
    /// Whether `folder` lies inside an archive opened as a folder. The
    /// answer is [`Event::ArchiveOf`], sent to the client that asked.
    ArchiveOf { folder: PathBuf },
}

/// What the archives core tells a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Whether an installed 7-Zip lets this system make, list and unpack
    /// 7z, and list and unpack rar. Zip and tar are always there.
    Formats { seven: bool },
    /// `folder` lies inside the archive file `archive`, or in no archive
    /// when `None`. What is inside an archive can be read but not changed.
    ArchiveOf {
        folder: PathBuf,
        archive: Option<PathBuf>,
    },
}

/// Unpack the archive `archive` into the folder `into`, as `to` says.
///
/// Zip, 7z, rar and tar, plain or compressed, unpack. It unpacks first
/// into a hidden folder in `into`, then moves what it unpacked into place
/// as a move does, asking about names already taken. A locked archive
/// asks [`PASSWORD_NEEDED`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extract {
    pub archive: PathBuf,
    pub into: PathBuf,
    pub to: ExtractTo,
}

/// Make an archive named `name` in the folder `into` of `sources`.
///
/// Each source goes in under its own name, a folder with all it holds,
/// packed as `packing` at `level`, from 1, fastest, to 9, smallest. A
/// name already taken gives the archive the next free name, as "keep
/// both" does. With `locked`, the job first asks [`PASSWORD_TO_LOCK`] for
/// the password that locks a zip's or a 7z's files; a tarball can't be
/// locked, and asking for one fails as unsupported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compress {
    pub sources: Vec<PathBuf>,
    pub into: PathBuf,
    pub name: OsString,
    pub packing: Packing,
    pub level: u8,
    pub locked: bool,
}

/// Where an archive's entries go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractTo {
    /// Straight into the folder when the archive holds one entry at its
    /// top; else into a new folder there named after the archive.
    Here,
    /// Straight into the folder.
    Inside,
}

/// What kind of archive a compress makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Packing {
    /// `.zip`, compressed with Deflate, which every system opens.
    ZipDeflate,
    /// `.zip`, compressed with Zstandard: faster and smaller, but older
    /// tools can't open it.
    ZipZstd,
    /// `.tar.zst`.
    TarZstd,
    /// `.tar.gz`.
    TarGzip,
    /// `.tar.xz`.
    TarXz,
    /// `.7z`, made by the installed 7-Zip: the smallest, and a password
    /// hides its names too.
    SevenZip,
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

/// The archive is locked. A window answers with a [`Password`] as the
/// reply, or skips it.
pub static PASSWORD_NEEDED: Question = Question::new("password needed", &[Answer::Retry]);

/// The password given doesn't open the archive; asked as
/// [`PASSWORD_NEEDED`] is.
pub static PASSWORD_WRONG: Question = Question::new("password wrong", &[Answer::Retry]);

/// A compress asks for the password that locks what it makes; asked as
/// [`PASSWORD_NEEDED`] is. An empty one is asked again.
pub static PASSWORD_TO_LOCK: Question = Question::new("password to lock", &[Answer::Retry]);

/// The reply to [`PASSWORD_NEEDED`], [`PASSWORD_WRONG`] or
/// [`PASSWORD_TO_LOCK`]. It never
/// shows in logs.
#[derive(Clone, PartialEq, Eq)]
pub struct Password(pub String);

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password(..)")
    }
}

#[cfg(test)]
mod tests {
    use nova_files_api::archive::ENDINGS;

    use super::*;

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

    #[test]
    fn a_password_never_shows_in_logs() {
        assert_eq!(
            format!("{:?}", Password("hunter2".to_owned())),
            "Password(..)"
        );
    }
}
