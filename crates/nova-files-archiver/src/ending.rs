use std::io;
use std::process::ExitCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Ending {
    Done = 0,
    Usage = 1,
    Broken = 4,
    Unsupported = 5,
    Gone = 6,
    Denied = 7,
    NeedsPassword = 8,
    WrongPassword = 9,
    NoSpace = 10,
    ReadOnly = 11,
    NotWritten = 12,
}

impl Ending {
    pub fn of_open(error: &io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => Self::Gone,
            io::ErrorKind::PermissionDenied => Self::Denied,
            _ => Self::Broken,
        }
    }

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
