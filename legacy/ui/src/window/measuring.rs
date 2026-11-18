use std::path::Path;

use gpui::Context;
use nova_files_api::{Command, MeasureId, ScanState, Size};
use nova_files_drives_api::DiskSpace;

use super::MainWindow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measure {
    id: MeasureId,
    pub size: Size,
    pub state: ScanState,
    before: Option<Size>,
}

impl Measure {
    pub const fn shown(&self) -> Size {
        match (self.state, self.before) {
            (ScanState::Counting, Some(before)) => before,
            _ => self.size,
        }
    }
}

impl MainWindow {
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

    pub fn measured(&self) -> Option<Measure> {
        self.properties_page()?.measure
    }

    pub(crate) fn drive_space_of(&self, path: &Path) -> Option<DiskSpace> {
        self.shown_drives()
            .into_iter()
            .filter_map(|drive| drive.folder)
            .filter(|folder| path.starts_with(folder))
            .max_by_key(|folder| folder.components().count())
            .and_then(|folder| self.disk_space(&folder))
    }
}
