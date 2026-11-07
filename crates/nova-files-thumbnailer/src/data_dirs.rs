//! The freedesktop data folders, most important first: `$XDG_DATA_HOME`
//! (or `~/.local/share`), then each of `$XDG_DATA_DIRS` (or
//! `/usr/local/share:/usr/share`). MIME types and thumbnailers are looked
//! up there, so whatever the system has installed is found.

use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

const DATA_HOME_VAR: &str = "XDG_DATA_HOME";
const DATA_DIRS_VAR: &str = "XDG_DATA_DIRS";
const DATA_IN_HOME: &str = ".local/share";
const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";

pub fn data_dirs() -> Vec<PathBuf> {
    from(
        env::var_os(DATA_HOME_VAR),
        env::var_os(DATA_DIRS_VAR),
        env::home_dir(),
    )
}

/// The data folders for these values of the two variables and the home
/// folder.
fn from(
    data_home: Option<OsString>,
    data_dirs: Option<OsString>,
    home: Option<PathBuf>,
) -> Vec<PathBuf> {
    let home = data_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| home.map(|home| home.join(DATA_IN_HOME)));
    let dirs = data_dirs
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| DEFAULT_DATA_DIRS.into());
    home.into_iter()
        .chain(env::split_paths(&dirs).filter(|path| path.is_absolute()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_or_empty_variables_use_the_defaults() {
        let home = Some(PathBuf::from("/home/someone"));
        let defaults = [
            "/home/someone/.local/share",
            "/usr/local/share",
            "/usr/share",
        ]
        .map(PathBuf::from);
        assert_eq!(from(None, None, home.clone()), defaults);
        assert_eq!(from(None, Some(OsString::new()), home), defaults);
    }

    #[test]
    fn set_variables_win_and_relative_folders_are_skipped() {
        let dirs = from(
            Some("/data".into()),
            Some("/a:relative:/b".into()),
            Some(PathBuf::from("/home/someone")),
        );
        assert_eq!(dirs, ["/data", "/a", "/b"].map(PathBuf::from));
        let relative_home = from(Some("relative".into()), Some("/a".into()), None);
        assert_eq!(relative_home, [PathBuf::from("/a")]);
    }
}
