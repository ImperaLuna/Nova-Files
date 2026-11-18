//! How a run ended, as the helper's exit code. The core reads the same
//! numbers (`nova-files-core`'s `archive::helper`); change both together.

use std::io;
use std::process::ExitCode;

/// Why the helper stopped; the number is the exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Ending {
    /// Everything asked for was done.
    Done = 0,
    /// Bad arguments, no sandbox, or nobody reading the answers.
    Usage = 1,
    /// The archive is damaged or cut short.
    Broken = 4,
    /// Not an archive the helper reads.
    Unsupported = 5,
    /// No file at that path.
    Gone = 6,
    /// The user may not read the archive, or write where it unpacks.
    Denied = 7,
    /// An entry is locked and no password was given.
    NeedsPassword = 8,
    /// The password given doesn't open an entry.
    WrongPassword = 9,
    /// The drive it unpacks to is full.
    NoSpace = 10,
    /// The drive it unpacks to is mounted read-only.
    ReadOnly = 11,
    /// Writing failed for any other reason.
    NotWritten = 12,
}

impl Ending {
    /// Why the archive didn't open.
    pub fn of_open(error: &io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => Self::Gone,
            io::ErrorKind::PermissionDenied => Self::Denied,
            _ => Self::Broken,
        }
    }

    /// Why an entry couldn't be written.
    pub fn of_write(error: &io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => Self::NoSpace,
            io::ErrorKind::ReadOnlyFilesystem => Self::ReadOnly,
            io::ErrorKind::PermissionDenied => Self::Denied,
            _ => Self::NotWritten,
        }
    }

    pub fn code(self) -> ExitCode {
        ExitCode::from(self as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_errors_say_why() {
        let of = |kind| Ending::of_open(&io::Error::from(kind));
        assert_eq!(of(io::ErrorKind::NotFound), Ending::Gone);
        assert_eq!(of(io::ErrorKind::PermissionDenied), Ending::Denied);
        assert_eq!(of(io::ErrorKind::IsADirectory), Ending::Broken);
    }

    #[test]
    fn write_errors_say_why() {
        let of = |kind| Ending::of_write(&io::Error::from(kind));
        assert_eq!(of(io::ErrorKind::StorageFull), Ending::NoSpace);
        assert_eq!(of(io::ErrorKind::QuotaExceeded), Ending::NoSpace);
        assert_eq!(of(io::ErrorKind::ReadOnlyFilesystem), Ending::ReadOnly);
        assert_eq!(of(io::ErrorKind::PermissionDenied), Ending::Denied);
        assert_eq!(of(io::ErrorKind::InvalidFilename), Ending::NotWritten);
    }
}
