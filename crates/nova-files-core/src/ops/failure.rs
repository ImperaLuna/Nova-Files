//! What a failed step tells the client: the system's error, sorted into
//! the few reasons a window shows differently.

use std::io;

use nova_files_api::Failure;

/// The reason `error` stands for.
pub fn failure(error: &io::Error) -> Failure {
    match error.kind() {
        io::ErrorKind::AlreadyExists | io::ErrorKind::DirectoryNotEmpty => Failure::NameTaken,
        io::ErrorKind::NotFound => Failure::NotFound,
        io::ErrorKind::PermissionDenied => Failure::PermissionDenied,
        io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => Failure::NoSpace,
        io::ErrorKind::ReadOnlyFilesystem => Failure::ReadOnly,
        io::ErrorKind::InvalidFilename => Failure::InvalidName,
        _ => Failure::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rustix::io::Errno;

    fn reason(errno: Errno) -> Failure {
        failure(&io::Error::from(errno))
    }

    #[test]
    fn sorts_the_errors_a_window_shows_apart() {
        assert_eq!(reason(Errno::EXIST), Failure::NameTaken);
        assert_eq!(reason(Errno::NOTEMPTY), Failure::NameTaken);
        assert_eq!(reason(Errno::NOENT), Failure::NotFound);
        assert_eq!(reason(Errno::ACCESS), Failure::PermissionDenied);
        assert_eq!(reason(Errno::PERM), Failure::PermissionDenied);
        assert_eq!(reason(Errno::NOSPC), Failure::NoSpace);
        assert_eq!(reason(Errno::DQUOT), Failure::NoSpace);
        assert_eq!(reason(Errno::ROFS), Failure::ReadOnly);
        assert_eq!(reason(Errno::NAMETOOLONG), Failure::InvalidName);
        assert_eq!(reason(Errno::IO), Failure::Other);
    }
}
