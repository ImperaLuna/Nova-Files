use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const CONFIG_HOME_VAR: &str = "XDG_CONFIG_HOME";
const CONFIG_IN_HOME: &str = ".config";
const STATE_HOME_VAR: &str = "XDG_STATE_HOME";
const STATE_IN_HOME: &str = ".local/state";
const DATA_HOME_VAR: &str = "XDG_DATA_HOME";
const DATA_IN_HOME: &str = ".local/share";
const CACHE_HOME_VAR: &str = "XDG_CACHE_HOME";
const CACHE_IN_HOME: &str = ".cache";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Home {
    pub folder: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
    pub data: PathBuf,
    pub drive_trashes: bool,
}

impl Home {
    pub fn from_env() -> Option<Self> {
        let folder = env::home_dir()?;
        let config = xdg_home(&folder, env::var_os(CONFIG_HOME_VAR), CONFIG_IN_HOME);
        let state = xdg_home(&folder, env::var_os(STATE_HOME_VAR), STATE_IN_HOME);
        let data = xdg_home(&folder, env::var_os(DATA_HOME_VAR), DATA_IN_HOME);
        Some(Self {
            folder,
            config,
            state,
            data,
            drive_trashes: true,
        })
    }
}

pub fn cache_home() -> Option<PathBuf> {
    cache_home_from(env::var_os(CACHE_HOME_VAR), env::home_dir())
}

fn cache_home_from(variable: Option<OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    variable
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| home.map(|home| home.join(CACHE_IN_HOME)))
}

fn xdg_home(home: &Path, from_env: Option<OsString>, in_home: &str) -> PathBuf {
    from_env
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(in_home))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/someone";

    #[test]
    fn xdg_folders_come_from_an_absolute_env_value_only() {
        let home = Path::new(HOME);
        assert_eq!(
            xdg_home(home, Some(OsString::from("/etc/xdg-me")), CONFIG_IN_HOME),
            PathBuf::from("/etc/xdg-me")
        );
        assert_eq!(
            xdg_home(home, Some(OsString::from("relative")), CONFIG_IN_HOME),
            PathBuf::from("/home/someone/.config")
        );
        assert_eq!(
            xdg_home(home, None, STATE_IN_HOME),
            PathBuf::from("/home/someone/.local/state")
        );
    }

    #[test]
    fn cache_home_prefers_an_absolute_variable() {
        let home = Some(PathBuf::from("/home/me"));
        assert_eq!(
            cache_home_from(Some("/var/cache/me".into()), home.clone()),
            Some(PathBuf::from("/var/cache/me"))
        );
        assert_eq!(
            cache_home_from(Some("relative".into()), home.clone()),
            Some(PathBuf::from("/home/me/.cache"))
        );
        assert_eq!(
            cache_home_from(None, home),
            Some(PathBuf::from("/home/me/.cache"))
        );
        assert_eq!(cache_home_from(None, None), None);
    }

    #[test]
    fn cache_home_reads_the_environment() {
        assert_eq!(
            cache_home(),
            cache_home_from(env::var_os(CACHE_HOME_VAR), env::home_dir())
        );
        assert!(cache_home().is_some_and(|cache| cache.is_absolute()));
    }
}
