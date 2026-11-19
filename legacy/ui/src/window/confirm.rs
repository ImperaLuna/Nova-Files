use gpui::{App, Context};
use nova_files_api::Operation;

use super::MainWindow;
use crate::island::Confirm;

const DELETE_LABEL: &str = "Delete";

impl MainWindow {
    pub(super) fn ask(&mut self, label: String, operation: Operation, cx: &mut Context<Self>) {
        let window = cx.entity().downgrade();
        let on_yes = move |cx: &mut App| {
            window
                .update(cx, |this, cx| {
                    let deletes = matches!(
                        operation,
                        Operation::Delete { .. } | Operation::DeleteFromTrash { .. }
                    );
                    if !deletes || this.deleting_selected().as_ref() == Some(&operation) {
                        this.start_job(operation);
                    }
                    cx.notify();
                })
                .ok();
        };
        self.island.confirm = Some(Confirm {
            label: label.into(),
            yes: DELETE_LABEL,
            on_yes: Box::new(on_yes),
        });
        cx.notify();
    }

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
