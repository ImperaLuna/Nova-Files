#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use gpui::{App, Context, Subscription, Window};
use nova_files_api::Entry;
use nova_files_api::uri::file_uri;
use nova_files_open_api::{Command, Event, NotOpened};
use nova_files_shell::assets::Icon;
use nova_files_shell::{
    Host, Location, MenuAt, MenuGroup, MenuItem, NoteGone, NoteId, Part, Placed, Scene, Slots,
    command_spec,
};

mod actions {
    #![expect(
        clippy::derive_partial_eq_without_eq,
        reason = "GPUI's actions! macro derives PartialEq only"
    )]
    gpui::actions!(nova_files_open, [OpenTerminalHere]);
}

pub use actions::OpenTerminalHere;

command_spec!(
    OPEN_TERMINAL,
    OpenTerminalHere,
    "folder.open-terminal",
    "Open terminal here",
    ["shift-f4"],
    needs[TakeOut]
);

const MOST_NAMES: usize = 3;
const FILE: &str = "file";
const FILES: &str = "files";
const LIST_GAP: &str = ", ";
const ROOT: &str = "/";
const NO_TERMINAL: &str = "No terminal found";
const TERMINAL_FAILED: &str = "Can't open a terminal here";

pub struct OpenPart {
    host: Host,
    failed: Vec<PathBuf>,
    shown: Option<NoteId>,
    _gone: Subscription,
}

impl OpenPart {
    fn open_file(&mut self, _: &Entry, path: &Path, cx: &mut Context<Self>) -> bool {
        let asked = self.host.link.send(Command::OpenFile {
            path: path.to_owned(),
            activation: None,
        });
        if asked.is_err() {
            hand_to_desktop(path, cx);
        }
        true
    }

    fn open_terminal(&mut self, _: &OpenTerminalHere, _: &mut Window, cx: &mut Context<Self>) {
        let Some(folder) = here(&self.host.scene(cx)) else {
            return;
        };
        let asked = self.host.link.send(Command::OpenTerminal {
            folder,
            activation: None,
        });
        if asked.is_err() {
            self.terminal_failed(NotOpened::Failed, cx);
        }
    }

    fn terminal_failed(&self, reason: NotOpened, cx: &mut Context<Self>) {
        let label = terminal_note(reason);
        self.host
            .island
            .update(cx, |island, cx| island.note(label.to_owned(), None, cx));
    }

    fn opened(&mut self, event: Event, cx: &mut Context<Self>) {
        let (path, reason) = match event {
            Event::NotOpened { path, reason } => (path, reason),
            Event::TerminalNotOpened { reason, .. } => {
                self.terminal_failed(reason, cx);
                return;
            }
            Event::Opened { .. } | Event::OpenWith(_) => return,
        };
        if reason != NotOpened::Failed {
            hand_to_desktop(&path, cx);
            return;
        }
        self.failed.retain(|was| *was != path);
        self.failed.push(path);
        if let Some(label) = note(&self.failed) {
            let id = self
                .host
                .island
                .update(cx, |island, cx| island.note(label, None, cx));
            self.shown = Some(id);
        }
    }

    fn note_gone(&mut self, id: NoteId) {
        if self.shown == Some(id) {
            self.shown = None;
            self.failed.clear();
        }
    }
}

impl Part for OpenPart {
    fn open(host: &Host, cx: &mut Context<Self>) -> Self {
        Self {
            host: host.clone(),
            failed: Vec::new(),
            shown: None,
            _gone: cx.subscribe(&host.island, |part, _, NoteGone(id), _| part.note_gone(*id)),
        }
    }

    fn slots(slots: &mut Slots<Self>) {
        slots.command(&OPEN_TERMINAL, in_folder, Self::open_terminal);
        slots.menu_in(MenuAt::Sheet, MenuGroup::Edit, Placed::LAST, terminal_item);
        slots.on_event(Self::opened);
        slots.opens(Self::open_file);
        slots.sends::<Command>();
    }
}

fn here(scene: &Scene) -> Option<PathBuf> {
    if scene.picker {
        return None;
    }
    scene
        .location
        .as_ref()
        .and_then(Location::folder)
        .map(Path::to_path_buf)
}

fn in_folder(_: &OpenPart, scene: &Scene, _: &App) -> bool {
    here(scene).is_some()
}

fn terminal_item(_: &OpenPart, scene: &Scene, _: &App) -> Vec<MenuItem> {
    if here(scene).is_none() || scene.searching {
        return Vec::new();
    }
    vec![MenuItem::run(Icon::Terminal, &OPEN_TERMINAL)]
}

const fn terminal_note(reason: NotOpened) -> &'static str {
    match reason {
        NotOpened::NoApp | NotOpened::Sandboxed => NO_TERMINAL,
        NotOpened::Failed => TERMINAL_FAILED,
    }
}

fn hand_to_desktop(path: &Path, cx: &Context<OpenPart>) {
    cx.open_url(&file_uri(path));
}

fn note(failed: &[PathBuf]) -> Option<String> {
    let names = if failed.len() > MOST_NAMES {
        count(failed.len())
    } else {
        let names: Vec<_> = failed.iter().map(|path| name_of(path)).collect();
        names.join(LIST_GAP)
    };
    let why = if failed.len() > 1 {
        "their apps didn't start"
    } else {
        "its app didn't start"
    };
    (!failed.is_empty()).then(|| format!("Can't open {names}: {why}"))
}

fn count(number: usize) -> String {
    if number == 1 {
        format!("1 {FILE}")
    } else {
        format!("{number} {FILES}")
    }
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || ROOT.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|name| PathBuf::from("/home/a").join(name))
            .collect()
    }

    #[test]
    fn one_file_says_its_name_and_why() {
        assert_eq!(note(&[]), None);
        assert_eq!(
            note(&failed(&["a.pdf"])).as_deref(),
            Some("Can't open a.pdf: its app didn't start")
        );
    }

    #[test]
    fn several_files_say_their_names_until_there_are_too_many() {
        assert_eq!(
            note(&failed(&["a", "b", "c"])).as_deref(),
            Some("Can't open a, b, c: their apps didn't start")
        );
        assert_eq!(
            note(&failed(&["a", "b", "c", "d"])).as_deref(),
            Some("Can't open 4 files: their apps didn't start")
        );
    }
}
