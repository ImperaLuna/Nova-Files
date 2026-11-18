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
    /// The user may not read it.
    Denied = 7,
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
}
