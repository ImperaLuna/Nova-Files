use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use nova_files_api::EventSink;
use nova_files_open_api::{Event, NotOpened};

use crate::defaults::{APPLICATIONS, Places};
use crate::entry;
use crate::exec::{Fields, bare_line, command_lines};
use crate::{FLATPAK_INFO, environment_places, launch};

const TERMINAL_VAR: &str = "TERMINAL";
const TERMINAL_EXEC: &str = "xdg-terminal-exec";
const DESKTOP_ENDING: &str = "desktop";
const TERMINAL_THREAD: &str = "nova-files-terminal";
const ANY_FOLDER: &str = "/";

pub fn open_terminal(events: &Arc<dyn EventSink>, folder: PathBuf, activation: Option<String>) {
    let thread_events = Arc::clone(events);
    let failed_folder = folder.clone();
    let spawned = thread::Builder::new()
        .name(TERMINAL_THREAD.to_owned())
        .spawn(move || {
            if let Err(reason) = start_in(&folder, activation.as_deref()) {
                thread_events
                    .deliver(Event::TerminalNotOpened { folder, reason })
                    .ok();
            }
        });
    if let Err(error) = spawned {
        log::warn!("can't start a thread to open a terminal: {error}");
        events
            .deliver(Event::TerminalNotOpened {
                folder: failed_folder,
                reason: NotOpened::Failed,
            })
            .ok();
    }
}

fn start_in(folder: &Path, activation: Option<&str>) -> Result<(), NotOpened> {
    if Path::new(FLATPAK_INFO).exists() {
        return Err(NotOpened::Sandboxed);
    }
    if !folder.is_dir() {
        return Err(NotOpened::Failed);
    }
    let chosen = std::env::var_os(TERMINAL_VAR);
    let line = terminal_line(&environment_places(), chosen.as_deref()).ok_or(NotOpened::NoApp)?;
    launch(&line, Some(folder), activation).map_err(|_| NotOpened::Failed)
}

fn terminal_line(places: &Places, chosen: Option<&OsStr>) -> Option<Vec<OsString>> {
    let search_path = places.search_path.as_deref();
    let program = |name: &str| entry::installed(name, search_path).then(|| vec![name.into()]);
    chosen
        .and_then(OsStr::to_str)
        .filter(|name| !name.is_empty())
        .and_then(program)
        .or_else(|| program(TERMINAL_EXEC))
        .or_else(|| listed_terminal(places))
}

fn listed_terminal(places: &Places) -> Option<Vec<OsString>> {
    places.data.iter().find_map(|data| {
        let mut files: Vec<PathBuf> = fs::read_dir(data.join(APPLICATIONS))
            .ok()?
            .filter_map(|found| Some(found.ok()?.path()))
            .filter(|file| file.extension() == Some(OsStr::new(DESKTOP_ENDING)))
            .collect();
        files.sort();
        files.iter().find_map(|file| {
            let app = entry::read(file, places.search_path.as_deref())?;
            let fields = Fields {
                name: &app.name,
                icon: app.icon.as_deref(),
                entry: file,
            };
            let takes_files = command_lines(&app.exec, &[Path::new(ANY_FOLDER)], fields).is_some();
            (app.emulates_terminal && app.listed && !takes_files)
                .then(|| bare_line(&app.exec, fields))
                .flatten()
        })
    })
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;
    use crate::tree::Tree;

    const RUNS: u32 = 0o755;

    fn program(tree: &Tree, name: &str) {
        tree.write(&format!("bin/{name}"), "");
        let path = tree.0.join("bin").join(name);
        fs::set_permissions(path, fs::Permissions::from_mode(RUNS)).unwrap();
    }

    fn line(tree: &Tree, chosen: Option<&str>) -> Option<Vec<String>> {
        let places = Places {
            search_path: Some(tree.0.join("bin").into_os_string()),
            ..tree.places()
        };
        terminal_line(&places, chosen.map(OsStr::new)).map(|line| {
            line.into_iter()
                .map(|word| word.to_string_lossy().into_owned())
                .collect()
        })
    }

    #[test]
    fn the_chosen_terminal_wins_then_the_spec_launcher_then_a_listed_app() {
        let tree = Tree::new("terminal-order");
        assert_eq!(line(&tree, Some("foot")), None);
        tree.write(
            "share/applications/b-term.desktop",
            "[Desktop Entry]\nType=Application\nExec=bterm %i --here\nIcon=bt\nCategories=System;TerminalEmulator;\n",
        )
        .write(
            "share/applications/b-term-open.desktop",
            "[Desktop Entry]\nType=Application\nExec=bterm +open %U\nCategories=TerminalEmulator;\n",
        )
        .write(
            "share/applications/b-term-server.desktop",
            "[Desktop Entry]\nType=Application\nExec=bterm --server\nNoDisplay=true\nCategories=TerminalEmulator;\n",
        )
        .write(
            "share/applications/a-editor.desktop",
            "[Desktop Entry]\nType=Application\nExec=editor\nCategories=Utility;\n",
        );
        assert_eq!(
            line(&tree, None),
            Some(vec![
                "bterm".into(),
                "--icon".into(),
                "bt".into(),
                "--here".into()
            ])
        );
        program(&tree, TERMINAL_EXEC);
        assert_eq!(line(&tree, Some("foot")), Some(vec![TERMINAL_EXEC.into()]));
        program(&tree, "foot");
        assert_eq!(line(&tree, Some("foot")), Some(vec!["foot".into()]));
        assert_eq!(line(&tree, Some("")), Some(vec![TERMINAL_EXEC.into()]));
    }
}
