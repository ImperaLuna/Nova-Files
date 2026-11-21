use gpui::{Context, Window};
use nova_files_api::ConflictKind;
use nova_files_shell::Asking;

use super::MainWindow;

impl MainWindow {
    pub(crate) fn settle_question(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let asked = self
            .job_on_island(cx)
            .and_then(|(job, running)| Some((job, running.asking.clone()?)))
            .filter(|(_, conflict)| matches!(conflict.kind, ConflictKind::Asked(_)))
            .filter(|_| !self.picker());
        let shown = asked
            .as_ref()
            .map(|(job, conflict)| (*job, conflict.number));
        if shown == self.island.asked {
            return;
        }
        self.island.asked = shown;
        let asking = asked.as_ref().map(|(job, conflict)| Asking {
            job: *job,
            conflict,
        });
        self.parts.asked(asking, window, cx);
    }
}
