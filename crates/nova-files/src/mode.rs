//! How the program was started, from its command line.
//!
//! `nova-files` opens a window on the home folder, `nova-files <folder>`
//! (a path or a `file://` URI, as launchers pass it) on that folder, and
//! `nova-files --service` runs only the core. `nova-files --index <folder>`
//! builds the file index of that folder and exits, for the bench.

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::PathBuf;

use nova_files_api::uri;

/// D-Bus passes it when it starts the program for a launcher.
const SERVICE_FLAG: &str = "--service";
/// Builds the index of the folder after it, then exits.
const INDEX_FLAG: &str = "--index";
/// Anything else starting with this is a flag the program doesn't take.
const FLAG_PREFIX: &str = "-";
/// What the usage line calls the folder argument.
const FOLDER_ARGUMENT: &str = "folder";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Open a window on the folder, or on the home folder.
    Window(Option<PathBuf>),
    /// Run only the core until something asks for a window.
    Service,
    /// Build the file index of the folder, then exit.
    Index(PathBuf),
}

impl Mode {
    /// Reads the arguments after the program name. A folder may still be
    /// relative; [`Mode::resolve`] makes it absolute.
    ///
    /// # Errors
    /// [`UsageError`] for an unknown flag, more than one argument, or a URI
    /// that doesn't name a local path.
    pub fn from_args(mut args: impl Iterator<Item = OsString>) -> Result<Self, UsageError> {
        let mode = match args.next() {
            None => Self::Window(None),
            Some(flag) if flag == SERVICE_FLAG => Self::Service,
            Some(flag) if flag == INDEX_FLAG => {
                let missing = || UsageError(flag.clone());
                Self::Index(folder(args.next().ok_or_else(missing)?)?)
            }
            Some(other) => Self::Window(Some(folder(other)?)),
        };
        args.next().map_or(Ok(mode), |extra| Err(UsageError(extra)))
    }

    /// Makes a relative folder absolute, from `working_folder`, which is
    /// asked for only then: a launch without a relative folder works even
    /// when the working folder is gone.
    ///
    /// # Errors
    /// When `working_folder` fails.
    pub fn resolve(self, working_folder: impl FnOnce() -> io::Result<PathBuf>) -> io::Result<Self> {
        match self {
            Self::Window(Some(folder)) if folder.is_relative() => {
                Ok(Self::Window(Some(working_folder()?.join(folder))))
            }
            Self::Index(folder) if folder.is_relative() => {
                Ok(Self::Index(working_folder()?.join(folder)))
            }
            other => Ok(other),
        }
    }
}

fn folder(argument: OsString) -> Result<PathBuf, UsageError> {
    let Some(text) = argument.to_str() else {
        // Not UTF-8, so not a flag or a URI: a path.
        return Ok(PathBuf::from(argument));
    };
    if text.starts_with(FLAG_PREFIX) {
        return Err(UsageError(argument));
    }
    if has_uri_scheme(text) {
        return uri::path_from_file_uri(text).ok_or(UsageError(argument));
    }
    Ok(PathBuf::from(argument))
}

/// Whether `text` starts with a URI scheme and `://` (`file://…`), not
/// just holds `://` somewhere, as the path `/tmp/a://b` does.
fn has_uri_scheme(text: &str) -> bool {
    text.split_once("://").is_some_and(|(scheme, _)| {
        scheme.starts_with(|first: char| first.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|part| part.is_ascii_alphanumeric() || matches!(part, '+' | '-' | '.'))
    })
}

/// An argument the program doesn't take.
#[derive(Debug)]
pub struct UsageError(OsString);

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unexpected argument {}; usage: nova-files [{SERVICE_FLAG} | {INDEX_FLAG} <{FOLDER_ARGUMENT}> | <{FOLDER_ARGUMENT}>]",
            self.0.display()
        )
    }
}

impl std::error::Error for UsageError {}

#[cfg(test)]
mod tests {
    use super::*;

    const HERE: &str = "/home/me";

    fn mode(args: &[&str]) -> Result<Mode, UsageError> {
        let mode = Mode::from_args(args.iter().map(OsString::from))?;
        Ok(mode.resolve(|| Ok(PathBuf::from(HERE))).unwrap())
    }

    #[test]
    fn no_arguments_opens_a_window_on_home() {
        assert_eq!(mode(&[]).unwrap(), Mode::Window(None));
    }

    #[test]
    fn service_flag_runs_only_the_core() {
        assert_eq!(mode(&["--service"]).unwrap(), Mode::Service);
    }

    #[test]
    fn index_flag_indexes_the_folder_after_it() {
        assert_eq!(
            mode(&["--index", "/srv/bench"]).unwrap(),
            Mode::Index(PathBuf::from("/srv/bench"))
        );
        assert_eq!(
            mode(&["--index", "bench"]).unwrap(),
            Mode::Index(PathBuf::from("/home/me/bench"))
        );
        assert!(mode(&["--index"]).is_err());
        assert!(mode(&["--index", "/srv/bench", "extra"]).is_err());
        let gone = || Err(io::Error::from(io::ErrorKind::NotFound));
        assert!(Mode::Index(PathBuf::from("/srv")).resolve(gone).is_ok());
    }

    #[test]
    fn folder_opens_a_window_there() {
        assert_eq!(
            mode(&["/tmp"]).unwrap(),
            Mode::Window(Some(PathBuf::from("/tmp")))
        );
        assert_eq!(
            mode(&["Downloads"]).unwrap(),
            Mode::Window(Some(PathBuf::from("/home/me/Downloads")))
        );
        assert_eq!(
            mode(&["file:///tmp/a%20b"]).unwrap(),
            Mode::Window(Some(PathBuf::from("/tmp/a b")))
        );
    }

    #[test]
    fn a_path_holding_a_scheme_separator_stays_a_path() {
        assert_eq!(
            mode(&["/tmp/a://b"]).unwrap(),
            Mode::Window(Some(PathBuf::from("/tmp/a://b")))
        );
        assert_eq!(
            mode(&["./a://b"]).unwrap(),
            Mode::Window(Some(PathBuf::from("/home/me/./a://b")))
        );
        assert_eq!(
            mode(&["9p://x"]).unwrap(),
            Mode::Window(Some(PathBuf::from("/home/me/9p://x")))
        );
    }

    #[test]
    fn working_folder_is_only_asked_for_a_relative_folder() {
        let gone = || Err(io::Error::from(io::ErrorKind::NotFound));
        assert!(Mode::Window(None).resolve(gone).is_ok());
        assert!(Mode::Service.resolve(gone).is_ok());
        assert!(
            Mode::Window(Some(PathBuf::from("/tmp")))
                .resolve(gone)
                .is_ok()
        );
        assert!(
            Mode::Window(Some(PathBuf::from("tmp")))
                .resolve(gone)
                .is_err()
        );
    }

    #[test]
    fn anything_else_is_refused() {
        assert!(mode(&["--frobnicate"]).is_err());
        assert!(mode(&["--service", "extra"]).is_err());
        assert!(mode(&["/tmp", "/home"]).is_err());
        assert!(mode(&["https://example.com"]).is_err());
        assert!(mode(&["svn+ssh://host/x"]).is_err());
    }

    #[test]
    fn refusal_names_the_argument_and_the_usage() {
        let error = mode(&["--frobnicate"]).unwrap_err();
        assert_eq!(
            error.to_string(),
            "unexpected argument --frobnicate; usage: nova-files [--service | --index <folder> | <folder>]"
        );
    }
}
