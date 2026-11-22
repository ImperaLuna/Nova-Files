use std::path::PathBuf;

use gpui::{AnyWindowHandle, App, Window};
use nova_files_api::{Event, JobId, Listing, Operation, Outcome};

use crate::Host;

#[derive(Debug, Default)]
pub struct RenameWhenMade {
    made: Option<Made>,
}

#[derive(Debug)]
struct Made {
    path: PathBuf,
    window: AnyWindowHandle,
    makes: Makes,
    job: Option<JobId>,
}

pub type Makes = fn(&Operation) -> Option<PathBuf>;

impl RenameWhenMade {
    pub fn wait(
        &mut self,
        host: &Host,
        (path, makes): (PathBuf, Makes),
        window: &Window,
        cx: &mut App,
    ) {
        let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
            return;
        };
        let (folder, name) = (folder.to_path_buf(), name.to_os_string());
        host.entries.update(cx, |entries, cx| {
            entries.select_when_listed(folder, vec![name], cx);
        });
        self.made = Some(Made {
            path,
            window: window.window_handle(),
            makes,
            job: None,
        });
    }

    pub fn watch(&mut self, event: &Event) {
        let Some(made) = self.made.as_mut() else {
            return;
        };
        match event {
            Event::JobStarted { job, operation, .. }
                if (made.makes)(operation).as_deref() == Some(made.path.as_path()) =>
            {
                made.job = Some(*job);
            }
            Event::JobFinished { job, outcome, .. }
                if made.job == Some(*job) && *outcome != Outcome::Done =>
            {
                self.made = None;
            }
            _ => {}
        }
    }

    pub fn listed(&mut self, listing: &Listing) -> Option<AnyWindowHandle> {
        let made = self.made.as_ref()?;
        if made.path.parent() != Some(listing.path.as_path()) {
            self.made = None;
            return None;
        }
        let name = made.path.file_name()?;
        let shown = listing.entries.iter().any(|entry| entry.name == name);
        shown
            .then(|| self.made.take().map(|made| made.window))
            .flatten()
    }
}
