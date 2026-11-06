//! How the program was started, from its command line.

use std::ffi::OsString;
use std::fmt;

/// The one flag the program takes; D-Bus passes it when it starts the
/// program for a launcher.
const SERVICE_FLAG: &str = "--service";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Open a window.
    Window,
    /// Run only the core until something asks for a window.
    Service,
}

impl Mode {
    /// Reads the arguments after the program name.
    ///
    /// # Errors
    /// [`UsageError`] for anything but no arguments or `--service`.
    pub fn from_args(mut args: impl Iterator<Item = OsString>) -> Result<Self, UsageError> {
        let mode = match args.next() {
            None => Self::Window,
            Some(flag) if flag == SERVICE_FLAG => Self::Service,
            Some(other) => return Err(UsageError(other)),
        };
        args.next().map_or(Ok(mode), |extra| Err(UsageError(extra)))
    }
}

/// An argument the program doesn't take.
#[derive(Debug)]
pub struct UsageError(OsString);

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unexpected argument {}; usage: nova-files [{SERVICE_FLAG}]",
            self.0.display()
        )
    }
}

impl std::error::Error for UsageError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(args: &[&str]) -> Result<Mode, UsageError> {
        Mode::from_args(args.iter().map(OsString::from))
    }

    #[test]
    fn no_arguments_opens_a_window() {
        assert_eq!(mode(&[]).unwrap(), Mode::Window);
    }

    #[test]
    fn service_flag_runs_only_the_core() {
        assert_eq!(mode(&["--service"]).unwrap(), Mode::Service);
    }

    #[test]
    fn anything_else_is_refused() {
        assert!(mode(&["--frobnicate"]).is_err());
        assert!(mode(&["--service", "extra"]).is_err());
    }
}
