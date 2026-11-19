use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use nova_files_api::EventSink;
use nova_files_open_api::{App as ListedApp, Event, OpenWith, OpenWithAnswer, OpenWithId};

use super::apps::{self, Listed};
use super::associate::{self, NotSet};
use super::defaults::{Places, installed};
use super::{FLATPAK_INFO, commands, environment_places, launch};
use crate::mime::MimeTypes;
use nova_files_disk::hand_edited::lock;

const THREAD_NAME: &str = "nova-files-open-with";

type Request = (Arc<dyn EventSink>, OpenWith);

pub struct Worker {
    types: Arc<OnceLock<MimeTypes>>,
    queue: Mutex<Option<Sender<Request>>>,
}

impl Worker {
    pub fn new(types: Arc<OnceLock<MimeTypes>>) -> Self {
        Self {
            types,
            queue: Mutex::new(None),
        }
    }

    pub fn answer(&self, events: &Arc<dyn EventSink>, request: OpenWith) {
        let Some(request) = self.queue(events, request) else {
            return;
        };
        log::warn!("can't start the Open With worker");
        let answer = match request {
            OpenWith::Apps { id, .. } => OpenWithAnswer::Apps {
                id,
                mime: None,
                apps: Vec::new(),
            },
            OpenWith::Launch { id, .. } => OpenWithAnswer::Launched { id, started: false },
            OpenWith::SetDefault { .. } => return,
        };
        events.deliver(Event::OpenWith(answer)).ok();
    }

    fn queue(&self, events: &Arc<dyn EventSink>, request: OpenWith) -> Option<OpenWith> {
        let mut queue = lock(&self.queue);
        let request = match queue.as_ref() {
            Some(sender) => match sender.send((Arc::clone(events), request)) {
                Ok(()) => return None,
                Err(mpsc::SendError((_, request))) => request,
            },
            None => request,
        };
        *queue = start(Arc::clone(&self.types));
        let unsent = match queue.as_ref() {
            Some(sender) => sender
                .send((Arc::clone(events), request))
                .err()
                .map(|mpsc::SendError((_, request))| request),
            None => Some(request),
        };
        if unsent.is_some() {
            *queue = None;
        }
        unsent
    }
}

fn start(types: Arc<OnceLock<MimeTypes>>) -> Option<Sender<Request>> {
    let (sender, requests) = mpsc::channel::<Request>();
    thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            for (events, request) in requests {
                if let Some(event) = handle(request, &types) {
                    events.deliver(event).ok();
                }
            }
        })
        .ok()?;
    Some(sender)
}

fn handle(request: OpenWith, types: &OnceLock<MimeTypes>) -> Option<Event> {
    let places = environment_places();
    let answer = match request {
        OpenWith::Apps { id, paths } => {
            let types = types.get_or_init(MimeTypes::from_env);
            list(id, &paths, types, &places)
        }
        OpenWith::Launch {
            id,
            paths,
            app,
            activation,
        } => OpenWithAnswer::Launched {
            id,
            started: open_with(&paths, &app, activation.as_deref(), &places),
        },
        OpenWith::SetDefault { mime, app } => {
            if let Err(error) = associate::set_default(&mime, &app, &places) {
                log::warn!("{app} isn't the default for {mime}: {}", reason(&error));
            }
            return None;
        }
    };
    Some(Event::OpenWith(answer))
}

fn list(id: OpenWithId, paths: &[PathBuf], types: &MimeTypes, places: &Places) -> OpenWithAnswer {
    let paths: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
    let shared = apps::shared_types(&paths, types);
    let ancestry: Vec<&str> = shared.ancestry.iter().map(String::as_str).collect();
    let listed = if ancestry.is_empty() {
        apps::apps_for_each(&apps::ancestries(&paths, types), places)
    } else {
        apps::apps(&ancestry, shared.same, places)
    };
    let apps = listed
        .into_iter()
        .map(|Listed { app, default }| ListedApp {
            id: app.id,
            name: app.entry.name,
            icon: app.entry.icon,
            default,
        })
        .collect();
    OpenWithAnswer::Apps {
        id,
        mime: shared
            .same
            .then(|| shared.ancestry.into_iter().next())
            .flatten(),
        apps,
    }
}

fn open_with(paths: &[PathBuf], id: &str, activation: Option<&str>, places: &Places) -> bool {
    if Path::new(FLATPAK_INFO).exists() || paths.iter().any(|path| !path.is_absolute()) {
        return false;
    }
    let files: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
    let Some((app, lines)) =
        installed(id, places).and_then(|app| commands(&app, &files).map(|lines| (app, lines)))
    else {
        return false;
    };
    let mut started = false;
    for line in &lines {
        let token = if started { None } else { activation };
        match launch(line, app.entry.working_dir.as_deref(), token) {
            Ok(()) => started = true,
            Err(error) => log::warn!("{id} didn't start: {error}"),
        }
    }
    started
}

fn reason(error: &NotSet) -> String {
    match error {
        NotSet::Invalid => "not a type and an installed app".to_owned(),
        NotSet::NoHome => "no config folder".to_owned(),
        NotSet::Failed(error) => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_of_types_with_no_parent_in_common_list_the_apps_that_open_each() {
        let tree = crate::tree::Tree::new("with-mixed");
        tree.write(
            "share/mime/globs2",
            "50:image/png:*.png\n50:image/jpeg:*.jpg\n",
        )
        .write(
            "share/applications/mimeinfo.cache",
            "[MIME Cache]\nimage/png=viewer.desktop;png-only.desktop\nimage/jpeg=viewer.desktop\n",
        );
        for id in ["viewer.desktop", "png-only.desktop"] {
            tree.write(
                &format!("share/applications/{id}"),
                "[Desktop Entry]\nType=Application\nName=app\nExec=app %F\n",
            );
        }
        for name in ["a.png", "b.jpg"] {
            tree.write(name, "");
        }
        let types = MimeTypes::load(&[tree.0.join("share")]);
        let paths = [tree.0.join("a.png"), tree.0.join("b.jpg")];

        let OpenWithAnswer::Apps { apps, mime, .. } =
            list(OpenWithId(1), &paths, &types, &tree.places())
        else {
            panic!("expected apps");
        };

        let ids: Vec<&str> = apps.iter().map(|app| app.id.as_str()).collect();
        assert_eq!(ids, ["viewer.desktop"]);
        assert_eq!(mime, None);
    }

    #[test]
    fn a_default_not_set_says_why() {
        assert_eq!(reason(&NotSet::Invalid), "not a type and an installed app");
        assert_eq!(reason(&NotSet::NoHome), "no config folder");
        let failed = NotSet::Failed(std::io::Error::other("disk full"));
        assert_eq!(reason(&failed), "disk full");
    }
}
