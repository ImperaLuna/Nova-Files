//! Measuring what a Properties page shows: a folder's size and what it
//! holds, or several paths' total, counted by the core's size scan.
//!
//! The page asks for a scan of all its paths each time it arrives or its
//! tab is shown again; its answers fill in a few times a second. Until a
//! scan ends, the page shows the size the scan before it ended at, if one
//! did. The core runs one size scan per window, so leaving the page
//! (another tab, a folder, Start) stops the scan. Each scan has its own
//! id, and the page takes answers for its newest one.
//!
//! The drive's share is worked out from the drive that holds the page's
//! first path, as the sidebar's drives know how big it is.

use std::path::Path;

use gpui::Context;
use nova_files_api::{Command, DiskSpace, MeasureId, ScanState, Size};

use super::MainWindow;

/// A Properties page's size scan and what it counted so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measure {
    id: MeasureId,
    pub size: Size,
    pub state: ScanState,
    /// What the scan before this one ended at.
    before: Option<Size>,
}

impl Measure {
    /// The size to show: what the scan before ended at while this one
    /// counts, else what this one counted.
    pub const fn shown(&self) -> Size {
        match (self.state, self.before) {
            (ScanState::Counting, Some(before)) => before,
            _ => self.size,
        }
    }
}

impl MainWindow {
    /// Starts measuring the Properties page's paths.
    pub(super) fn measure(&mut self) {
        let id = MeasureId(self.next_ask());
        let Some(page) = self.properties.as_mut() else {
            return;
        };
        let before = page
            .measure
            .filter(|measure| measure.state == ScanState::Done)
            .map(|measure| measure.size);
        let paths = page.paths();
        if paths.is_empty() {
            return;
        }
        let sent = self
            .connection
            .as_ref()
            .is_some_and(|connection| connection.send(Command::Measure { id, paths }).is_ok());
        page.measure = sent.then_some(Measure {
            id,
            size: Size::default(),
            state: ScanState::Counting,
            before,
        });
    }

    /// Stops the Properties page's scan if it hasn't ended.
    pub(super) fn stop_measuring(&mut self) {
        let Some(page) = self.properties.as_mut() else {
            return;
        };
        let Some(measure) = page
            .measure
            .take_if(|measure| measure.state == ScanState::Counting)
        else {
            return;
        };
        if let Some(connection) = &self.connection {
            connection.send(Command::StopMeasure(measure.id)).ok();
        }
    }

    /// What scan `id` counted so far.
    pub(super) fn size_measured(
        &mut self,
        id: MeasureId,
        size: Size,
        state: ScanState,
        cx: &mut Context<Self>,
    ) {
        let Some(measure) = self
            .properties
            .as_mut()
            .and_then(|page| page.measure.as_mut())
            .filter(|measure| measure.id == id)
        else {
            return;
        };
        measure.size = size;
        measure.state = state;
        cx.notify();
    }

    /// What the Properties page on screen measured so far, and the scan's
    /// state.
    pub fn measured(&self) -> Option<Measure> {
        self.properties_page()?.measure
    }

    /// How big the drive that holds `path` is and how much is free: the
    /// mounted drive whose folder is the nearest above it.
    pub(crate) fn drive_space_of(&self, path: &Path) -> Option<DiskSpace> {
        self.shown_drives()
            .into_iter()
            .filter_map(|drive| drive.folder)
            .filter(|folder| path.starts_with(folder))
            .max_by_key(|folder| folder.components().count())
            .and_then(|folder| self.disk_space(&folder))
    }
}
