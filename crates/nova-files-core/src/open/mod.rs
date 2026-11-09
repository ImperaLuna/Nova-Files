//! Opening a file in the user's default app, the way a desktop does it,
//! without the desktop portal: the portal asks which app to use for host
//! apps until the user has picked one, so unsandboxed it only stays as the
//! fallback (in the D-Bus service) for when this can't open the file.
//!
//! The default app comes from `mimeapps.list` and `mimeinfo.cache`
//! (`defaults`), its command from the desktop entry's `Exec` line (`exec`).
//! The app starts in its own process group with no input or output, and
//! gets the caller's activation token, so the compositor lets it take
//! focus.

mod defaults;
mod entry;
mod exec;

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;

use defaults::{App, Places};
use exec::Fields;
use nova_files_api::FileTypes as _;

use crate::mime::MimeTypes;

/// Present inside a Flatpak sandbox, where the host's apps can't be run.
const FLATPAK_INFO: &str = "/.flatpak-info";
/// The token Wayland compositors read (xdg-activation).
const ACTIVATION_TOKEN_VAR: &str = "XDG_ACTIVATION_TOKEN";
/// The same for X11 startup notification.
const STARTUP_ID_VAR: &str = "DESKTOP_STARTUP_ID";
const CURRENT_DESKTOP_VAR: &str = "XDG_CURRENT_DESKTOP";
const DESKTOP_SEPARATOR: char = ':';
const PATH_VAR: &str = "PATH";
const HOME_VAR: &str = "HOME";
const CONFIG_HOME_VAR: &str = "XDG_CONFIG_HOME";
const CONFIG_DIRS_VAR: &str = "XDG_CONFIG_DIRS";
const DATA_HOME_VAR: &str = "XDG_DATA_HOME";
const DATA_DIRS_VAR: &str = "XDG_DATA_DIRS";
const CONFIG_IN_HOME: &str = ".config";
const DATA_IN_HOME: &str = ".local/share";
const DEFAULT_CONFIG_DIRS: &str = "/etc/xdg";
const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";
const REAPER_THREAD: &str = "nova-files-opened-app";

/// Why a file wasn't opened here; the caller can still ask the portal.
#[derive(Debug)]
pub enum NotOpened {
    /// Inside a sandbox, so the portal must do it.
    Sandboxed,
    /// No installed app is the default for the file's type, or its
    /// `Exec` line can't take a file.
    NoApp,
    /// The app's program couldn't start.
    Failed,
}

/// Opens `file` in the default app for its type, as `types` tells it from
/// the file's name, or for the nearest type that one is a kind of.
///
/// # Errors
/// [`NotOpened`] says why nothing started; [`NotOpened::NoApp`] when the
/// name says no type, which the portal may still tell from the contents.
pub fn open_file(
    file: &Path,
    types: &MimeTypes,
    activation: Option<&str>,
) -> Result<(), NotOpened> {
    let ancestry = types_of(file, types);
    let ancestry: Vec<&str> = ancestry.iter().map(String::as_str).collect();
    if ancestry.is_empty() {
        return Err(NotOpened::NoApp);
    }
    open_in_default_app(file, &ancestry, activation)
}

/// `file`'s type and the types it is a kind of, nearest first; none when
/// its name says no type.
fn types_of(file: &Path, types: &MimeTypes) -> Vec<String> {
    file.file_name()
        .and_then(|name| types.of(name))
        .map(|mime| types.ancestry(mime))
        .unwrap_or_default()
}

/// Opens `file` in the default app for the first of `types` (its MIME
/// type, then the types that one is a kind of) that has one. `activation`
/// is the caller's activation token, if it has one.
///
/// # Errors
/// [`NotOpened`] says why nothing started.
pub fn open_in_default_app(
    file: &Path,
    types: &[&str],
    activation: Option<&str>,
) -> Result<(), NotOpened> {
    if Path::new(FLATPAK_INFO).exists() {
        return Err(NotOpened::Sandboxed);
    }
    let places = places_from(|name| std::env::var_os(name));
    let app = defaults::default_app(types, &places).ok_or(NotOpened::NoApp)?;
    let line = command(&app, file).ok_or(NotOpened::NoApp)?;
    launch(&line, app.entry.working_dir.as_deref(), activation).map_err(|_| NotOpened::Failed)
}

/// The app's command line for `file`.
fn command(app: &App, file: &Path) -> Option<Vec<OsString>> {
    let fields = Fields {
        name: &app.entry.name,
        icon: app.entry.icon.as_deref(),
        entry: &app.file,
    };
    exec::command_line(&app.entry.exec, file, fields)
}

/// Starts `line` on its own, and reaps it on a thread when it exits. The
/// thread starts first, so an app that runs is always reaped and reported
/// as started.
fn launch(
    line: &[OsString],
    working_dir: Option<&Path>,
    activation: Option<&str>,
) -> io::Result<()> {
    let (program, args) = line
        .split_first()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut command = prepare(program, args, working_dir, activation);
    let (hand_over, started) = mpsc::channel::<Child>();
    // Without the app, the sender is dropped and the thread ends at once.
    thread::Builder::new()
        .name(REAPER_THREAD.to_owned())
        .spawn(move || started.recv().map(|mut child| child.wait()))?;
    let child = command.spawn()?;
    // The thread only ends once it has the child, so this can't fail.
    hand_over.send(child).ok();
    Ok(())
}

/// The command for one launch: no input or output, its own process group,
/// and only the activation token passed here, never one this process got.
fn prepare(
    program: &OsStr,
    args: &[OsString],
    working_dir: Option<&Path>,
    activation: Option<&str>,
) -> Command {
    use std::os::unix::process::CommandExt as _;

    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .env_remove(ACTIVATION_TOKEN_VAR)
        .env_remove(STARTUP_ID_VAR);
    if let Some(dir) = working_dir {
        command.current_dir(dir);
    }
    if let Some(token) = activation {
        command
            .env(ACTIVATION_TOKEN_VAR, token)
            .env(STARTUP_ID_VAR, token);
    }
    command
}

/// The folders and desktop names the environment (`var`) names. Relative
/// paths are ignored, as the base directory spec says.
fn places_from(var: impl Fn(&str) -> Option<OsString>) -> Places {
    let home = var(HOME_VAR)
        .map(PathBuf::from)
        .filter(|home| home.is_absolute());
    let one = |name: &str, in_home: &str| {
        var(name)
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .or_else(|| home.as_ref().map(|home| home.join(in_home)))
    };
    let list = |name: &str, fallback: &str| {
        let value = var(name)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| fallback.into());
        std::env::split_paths(&value)
            .filter(|dir| dir.is_absolute())
            .collect::<Vec<_>>()
    };
    let desktops = var(CURRENT_DESKTOP_VAR)
        .map(|value| {
            value
                .to_string_lossy()
                .split(DESKTOP_SEPARATOR)
                .filter(|name| !name.is_empty())
                .map(str::to_lowercase)
                .collect()
        })
        .unwrap_or_default();
    Places {
        config: one(CONFIG_HOME_VAR, CONFIG_IN_HOME)
            .into_iter()
            .chain(list(CONFIG_DIRS_VAR, DEFAULT_CONFIG_DIRS))
            .collect(),
        data: one(DATA_HOME_VAR, DATA_IN_HOME)
            .into_iter()
            .chain(list(DATA_DIRS_VAR, DEFAULT_DATA_DIRS))
            .collect(),
        desktops,
        search_path: var(PATH_VAR),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;

    use super::*;

    fn places(vars: &[(&str, &str)]) -> Places {
        let vars: HashMap<&str, &str> = vars.iter().copied().collect();
        places_from(|name| vars.get(name).map(OsString::from))
    }

    fn paths(dirs: &[&str]) -> Vec<PathBuf> {
        dirs.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn the_environment_names_the_folders_and_desktops() {
        let found = places(&[
            ("HOME", "/home/me"),
            ("XDG_CONFIG_HOME", "/cfg"),
            ("XDG_CONFIG_DIRS", "/etc/a:rel:/etc/b"),
            ("XDG_DATA_HOME", "/data"),
            ("XDG_DATA_DIRS", "/run/share:/usr/share"),
            ("XDG_CURRENT_DESKTOP", "Hyprland::GNOME"),
            ("PATH", "/bin"),
        ]);
        assert_eq!(found.config, paths(&["/cfg", "/etc/a", "/etc/b"]));
        assert_eq!(found.data, paths(&["/data", "/run/share", "/usr/share"]));
        assert_eq!(found.desktops, ["hyprland", "gnome"]);
        assert_eq!(found.search_path, Some(OsString::from("/bin")));
    }

    #[test]
    fn unset_or_relative_folders_fall_back_to_the_defaults() {
        let found = places(&[
            ("HOME", "/home/me"),
            ("XDG_CONFIG_HOME", "rel"),
            ("XDG_DATA_DIRS", ""),
        ]);
        assert_eq!(found.config, paths(&["/home/me/.config", "/etc/xdg"]));
        assert_eq!(
            found.data,
            paths(&["/home/me/.local/share", "/usr/local/share", "/usr/share"])
        );
        assert_eq!(found.desktops, Vec::<String>::new());
        assert_eq!(found.search_path, None);
        let homeless = places(&[("HOME", "rel")]);
        assert_eq!(homeless.config, paths(&["/etc/xdg"]));
        assert_eq!(homeless.data, paths(&["/usr/local/share", "/usr/share"]));
    }

    #[test]
    fn a_launch_passes_only_the_given_token() {
        let command = prepare(
            OsStr::new("app"),
            &[OsString::from("/h/a")],
            Some(Path::new("/opt")),
            Some("tok"),
        );
        let envs: HashMap<_, _> = command.get_envs().collect();
        assert_eq!(
            envs.get(OsStr::new(ACTIVATION_TOKEN_VAR)),
            Some(&Some(OsStr::new("tok")))
        );
        assert_eq!(
            envs.get(OsStr::new(STARTUP_ID_VAR)),
            Some(&Some(OsStr::new("tok")))
        );
        assert_eq!(command.get_current_dir(), Some(Path::new("/opt")));
        assert_eq!(command.get_args().collect::<Vec<_>>(), [OsStr::new("/h/a")]);
        let command = prepare(OsStr::new("app"), &[], None, None);
        let envs: HashMap<_, _> = command.get_envs().collect();
        assert_eq!(envs.get(OsStr::new(ACTIVATION_TOKEN_VAR)), Some(&None));
        assert_eq!(envs.get(OsStr::new(STARTUP_ID_VAR)), Some(&None));
        assert_eq!(command.get_current_dir(), None);
    }

    #[test]
    fn a_launched_app_gets_the_file_and_token() {
        let root = std::env::temp_dir().join(format!("nova-open-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        // Reading a FIFO waits for the app to write it, with no polling.
        let out = root.join("out");
        let made = Command::new("mkfifo").arg(&out).status().unwrap();
        assert!(made.success());
        let script = format!(
            "printf '%s %s %s' \"$1\" \"$XDG_ACTIVATION_TOKEN\" \"$PWD\" > '{}'",
            out.display()
        );
        let line = [
            OsString::from("/bin/sh"),
            OsString::from("-c"),
            OsString::from(script),
            OsString::from("sh"),
            OsString::from("/h/a b.png"),
        ];
        assert!(launch(&[], None, None).is_err());
        assert!(launch(&[OsString::from("/nonexistent/app")], None, None).is_err());
        launch(&line, Some(&root), Some("tok")).unwrap();
        let written = fs::read_to_string(&out).unwrap();
        assert_eq!(written, format!("/h/a b.png tok {}", root.display()));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_type_nothing_opens_is_left_to_the_portal() {
        let opened =
            open_in_default_app(Path::new("/h/a"), &["application/x-nova-files-test"], None);
        assert!(
            matches!(opened, Err(NotOpened::Sandboxed | NotOpened::NoApp)),
            "{opened:?}"
        );
        let opened = open_in_default_app(Path::new("/h/a"), &[], None);
        assert!(
            matches!(opened, Err(NotOpened::Sandboxed | NotOpened::NoApp)),
            "{opened:?}"
        );
    }

    #[test]
    fn an_app_without_a_file_code_is_no_app() {
        let app = App {
            entry: entry::Entry {
                exec: "viewer --new".to_owned(),
                name: String::new(),
                icon: None,
                working_dir: None,
            },
            file: PathBuf::from("/a.desktop"),
        };
        assert_eq!(command(&app, Path::new("/h/a")), None);
        let app = App {
            entry: entry::Entry {
                exec: "viewer %k %f".to_owned(),
                ..app.entry
            },
            ..app
        };
        assert_eq!(
            command(&app, Path::new("/h/a")),
            Some(vec![
                OsString::from("viewer"),
                OsString::from("/a.desktop"),
                OsString::from("/h/a")
            ])
        );
    }
}
