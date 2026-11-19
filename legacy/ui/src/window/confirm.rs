use gpui::Context;

use super::MainWindow;
use crate::island::Confirm;

impl MainWindow {
    pub(crate) fn answer_confirm(&mut self, yes: bool, cx: &mut Context<Self>) {
        if let Some(confirm) = self.island.confirm.take()
            && yes
        {
            cx.defer(confirm.on_yes);
        }
        cx.notify();
    }

    pub const fn confirm(&self) -> Option<&Confirm> {
        self.island.confirm.as_ref()
    }
}
