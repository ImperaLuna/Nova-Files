use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use nova_files_api::{EventSink, Fit, Picture};
use nova_files_preview_api::markdown::FigureKind;
use nova_files_preview_api::{Event, PreviewId};
use nova_files_thumbnailer_launch::{self as launch, Running};

use crate::{Current, THREAD_NAME};

#[derive(Debug, Default)]
pub struct Drawing {
    turn: Mutex<()>,
    running: Running,
}

impl Drawing {
    pub fn stop(&self) {
        self.running.stop();
    }
}

pub struct Asked {
    pub id: PreviewId,
    pub document: PathBuf,
    pub picture: PathBuf,
    pub fit: Fit,
}

pub fn draw(
    events: &Arc<dyn EventSink>,
    current: &Arc<Current>,
    helper: Option<PathBuf>,
    asked: Asked,
) {
    let (id, picture) = (asked.id, asked.picture.clone());
    let failed = Event::Drawn {
        id,
        picture: picture.clone(),
        drawn: None,
    };
    answer_aside(events, current, (id, failed), move |current| {
        let drawn = helper
            .as_deref()
            .and_then(|helper| drawn(current, helper, &asked));
        Event::Drawn { id, picture, drawn }
    });
}

fn answer_aside(
    events: &Arc<dyn EventSink>,
    current: &Arc<Current>,
    (id, failed): (PreviewId, Event),
    work: impl FnOnce(&Current) -> Event + Send + 'static,
) {
    let sink = Arc::clone(events);
    let current = Arc::clone(current);
    let spawned = thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            let event = work(&current);
            if current.is(id) {
                sink.deliver(event).ok();
            }
        });
    if spawned.is_err() {
        events.deliver(failed).ok();
    }
}

pub struct FigureAsked {
    pub id: PreviewId,
    pub kind: FigureKind,
    pub source: Arc<str>,
    pub zoom_percent: u16,
    pub light: bool,
    pub fit: Fit,
}

pub fn figure(
    events: &Arc<dyn EventSink>,
    current: &Arc<Current>,
    helper: Option<PathBuf>,
    asked: FigureAsked,
) {
    let (id, source) = (asked.id, Arc::clone(&asked.source));
    let failed = Event::Figured {
        id,
        source: Arc::clone(&source),
        drawn: None,
    };
    answer_aside(events, current, (id, failed), move |current| {
        let drawn = helper
            .as_deref()
            .and_then(|helper| figured(current, helper, &asked));
        Event::Figured { id, source, drawn }
    });
}

fn figured(current: &Current, helper: &Path, asked: &FigureAsked) -> Option<Picture> {
    let drawing = &current.drawing;
    let _turn = drawing.turn.lock().unwrap_or_else(PoisonError::into_inner);
    let wanted = || current.is(asked.id);
    if !wanted() {
        return None;
    }
    match asked.kind {
        FigureKind::Diagram => {
            let source = (&*asked.source, asked.zoom_percent, asked.light);
            launch::diagram(helper, source, asked.fit, &drawing.running, wanted)
        }
        FigureKind::Formula => None,
    }
}

fn drawn(current: &Current, helper: &Path, asked: &Asked) -> Option<Picture> {
    let picture = beside(&asked.document, &asked.picture)?;
    let drawing = &current.drawing;
    let _turn = drawing.turn.lock().unwrap_or_else(PoisonError::into_inner);
    let wanted = || current.is(asked.id);
    if !wanted() {
        return None;
    }
    let frames = launch::picture(helper, &picture, asked.fit, &drawing.running, wanted)?;
    frames.into_iter().next().map(|(picture, _)| picture)
}

fn beside(document: &Path, picture: &Path) -> Option<PathBuf> {
    let folder = fs::canonicalize(document).ok()?.parent()?.to_path_buf();
    let picture = fs::canonicalize(picture).ok()?;
    (picture.starts_with(&folder) && picture.is_file()).then_some(picture)
}

#[cfg(test)]
mod tests {
    use nova_files_test_folder::TestFolder;

    use super::*;

    #[test]
    fn only_pictures_inside_the_documents_folder_are_drawn() {
        let folder = TestFolder::new("preview-drawing-beside");
        let notes = folder.join("notes");
        fs::create_dir_all(notes.join("art")).unwrap();
        let document = notes.join("a.md");
        fs::write(&document, "![x](art/x.png)").unwrap();
        fs::write(notes.join("art/x.png"), "png").unwrap();
        fs::write(folder.join("outside.png"), "png").unwrap();

        let inside = beside(&document, &notes.join("art/../art/x.png"));
        assert_eq!(
            inside,
            Some(fs::canonicalize(notes.join("art/x.png")).unwrap())
        );
        assert_eq!(beside(&document, &notes.join("../outside.png")), None);
        assert_eq!(beside(&document, &notes.join("art")), None);
        assert_eq!(beside(&document, &notes.join("missing.png")), None);
    }
}
