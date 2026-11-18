mod apps;
mod associate;
mod defaults;
mod entry;
mod exec;
mod folders;
mod mime;
#[cfg(test)]
mod tree;
mod with;

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, OnceLock, mpsc};
use std::thread;

use defaults::{App, Places};
use exec::Fields;
use nova_files_api::{EventSink, FileTypes as _};
pub use nova_files_open_api::NotOpened;
use nova_files_open_api::{Event, TakeOut};

pub use folders::{NotDefault, make_default_for_folders, opens_folders};
pub use mime::{MimeTypes, data_dirs};
pub use with::Worker as OpenWithWorker;

const FLATPAK_INFO: &str = "/.flatpak-info";
const ACTIVATION_TOKEN_VAR: &str = "XDG_ACTIVATION_TOKEN";
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
const OPEN_THREAD: &str = "nova-files-open";
const ANY_FILE: &str = "/";

pub fn open(
    events: &Arc<dyn EventSink>,
    (types, archives): (Arc<OnceLock<MimeTypes>>, Arc<dyn TakeOut>),
    path: PathBuf,
    activation: Option<String>,
) {
    let thread_events = Arc::clone(events);
    let failed_path = path.clone();
    let spawned = thread::Builder::new()
        .name(OPEN_THREAD.to_owned())
        .spawn(move || {
            let types = types.get_or_init(MimeTypes::from_env);
            let opened = match opened_copy(&path, archives.as_ref()) {
                Some(Ok(copy)) => open_file(&copy, types, activation.as_deref()),
                Some(Err(reason)) => Err(reason),
                None => open_file(&path, types, activation.as_deref()),
            };
            let event = match opened {
                Ok(()) => Event::Opened { path },
                Err(reason) => Event::NotOpened { path, reason },
            };
            thread_events.deliver(event).ok();
        });
    if let Err(error) = spawned {
        log::warn!("can't start a thread to open a file: {error}");
        events
            .deliver(Event::NotOpened {
                path: failed_path,
                reason: NotOpened::Failed,
            })
            .ok();
    }
}

fn opened_copy(path: &Path, archives: &dyn TakeOut) -> Option<Result<PathBuf, NotOpened>> {
    if path.symlink_metadata().is_ok() {
        return None;
    }
    archives.take_out(path)
}

fn open_file(file: &Path, types: &MimeTypes, activation: Option<&str>) -> Result<(), NotOpened> {
    let ancestry = types_of(file, types);
    let ancestry: Vec<&str> = ancestry.iter().map(String::as_str).collect();
    if ancestry.is_empty() {
        return Err(NotOpened::NoApp);
    }
    open_in_default_app(file, &ancestry, activation)
}

fn types_of(file: &Path, types: &MimeTypes) -> Vec<String> {
    file.file_name()
        .and_then(|name| types.of(name))
        .map(|mime| types.ancestry(mime))
        .unwrap_or_default()
}

pub fn open_in_default_app(
    file: &Path,
    types: &[&str],
    activation: Option<&str>,
) -> Result<(), NotOpened> {
    if Path::new(FLATPAK_INFO).exists() {
        return Err(NotOpened::Sandboxed);
    }
    open_from(file, types, activation, &environment_places())
}

fn environment_places() -> Places {
    places_from(|name| std::env::var_os(name))
}

fn open_from(
    file: &Path,
    types: &[&str],
    activation: Option<&str>,
    places: &Places,
) -> Result<(), NotOpened> {
    let app = defaults::default_app(types, places).ok_or(NotOpened::NoApp)?;
    let line = commands(&app, &[file])
        .and_then(|mut lines| lines.pop())
        .ok_or(NotOpened::NoApp)?;
    launch(&line, app.entry.working_dir.as_deref(), activation).map_err(|_| NotOpened::Failed)
}

fn commands(app: &App, files: &[&Path]) -> Option<Vec<Vec<OsString>>> {
    if app.entry.terminal {
        return None;
    }
    let fields = Fields {
        name: &app.entry.name,
        icon: app.entry.icon.as_deref(),
        entry: &app.file,
    };
    exec::command_lines(&app.entry.exec, files, fields)
}

fn opens_files(app: &App) -> bool {
    commands(app, &[Path::new(ANY_FILE)]).is_some()
}

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
    thread::Builder::new()
        .name(REAPER_THREAD.to_owned())
        .spawn(move || started.recv().map(|mut child| child.wait()))?;
    let child = command.spawn()?;
    hand_over.send(child).ok();
    Ok(())
}

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
    let config_home = one(CONFIG_HOME_VAR, CONFIG_IN_HOME);
    Places {
        config: config_home
            .iter()
            .cloned()
            .chain(list(CONFIG_DIRS_VAR, DEFAULT_CONFIG_DIRS))
            .collect(),
        config_home,
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
        assert_eq!(found.config_home, Some(PathBuf::from("/cfg")));
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
        assert_eq!(found.config_home, Some(PathBuf::from("/home/me/.config")));
        assert_eq!(
            found.data,
            paths(&["/home/me/.local/share", "/usr/local/share", "/usr/share"])
        );
        assert_eq!(found.desktops, Vec::<String>::new());
        assert_eq!(found.search_path, None);
        let homeless = places(&[("HOME", "rel")]);
        assert_eq!(homeless.config, paths(&["/etc/xdg"]));
        assert_eq!(homeless.config_home, None);
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
        let places = Places::default();
        let opened = open_from(Path::new("/h/a"), &["image/png"], None, &places);
        assert!(matches!(opened, Err(NotOpened::NoApp)), "{opened:?}");
        let opened = open_from(Path::new("/h/a"), &[], None, &places);
        assert!(matches!(opened, Err(NotOpened::NoApp)), "{opened:?}");
    }

    fn command(app: &App, file: &Path) -> Option<Vec<OsString>> {
        commands(app, &[file]).map(|mut lines| lines.remove(0))
    }

    #[test]
    fn an_app_without_a_file_code_is_no_app() {
        let app = App {
            id: "viewer.desktop".to_owned(),
            entry: entry::Entry {
                exec: "viewer --new".to_owned(),
                name: String::new(),
                icon: None,
                working_dir: None,
                terminal: false,
            },
            file: PathBuf::from("/a.desktop"),
        };
        assert_eq!(command(&app, Path::new("/h/a")), None);
        assert!(!opens_files(&app));
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
        assert!(opens_files(&app));
        let app = App {
            entry: entry::Entry {
                terminal: true,
                ..app.entry
            },
            ..app
        };
        assert_eq!(command(&app, Path::new("/h/a")), None);
        assert!(!opens_files(&app));
    }
}
