#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use gpui::{Context, Subscription};
use nova_files_api::Entry;
use nova_files_api::uri::file_uri;
use nova_files_open_api::{Command, Event, NotOpened};
use nova_files_shell::{Host, NoteGone, NoteId, Part, Slots};

const MOST_NAMES: usize = 3;
const FILE: &str = "file";
const FILES: &str = "files";
const LIST_GAP: &str = ", ";
const ROOT: &str = "/";

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

    fn opened(&mut self, event: Event, cx: &mut Context<Self>) {
        let Event::NotOpened { path, reason } = event else {
            return;
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
        slots.on_event(Self::opened);
        slots.opens(Self::open_file);
        slots.sends::<Command>();
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
