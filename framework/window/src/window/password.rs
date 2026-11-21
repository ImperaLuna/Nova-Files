use gpui::{AppContext as _, Context, Focusable as _, Window};
use nova_files_api::{Command, Conflict, ConflictKind, Operation};
use nova_files_shell::text::{TextField, TextFieldEvent};

use super::MainWindow;
use crate::island::Password;

const UNLOCK: &str = "Unlock";
const LOCK: &str = "Lock";

pub const fn asks_password(conflict: &Conflict) -> bool {
    matches!(
        conflict.kind,
        ConflictKind::PasswordNeeded | ConflictKind::PasswordWrong
    )
}

pub const fn password_label(operation: &Operation) -> &'static str {
    if matches!(operation, Operation::Compress { .. }) {
        LOCK
    } else {
        UNLOCK
    }
}

impl MainWindow {
    pub(crate) fn settle_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let asked = self.job_on_island(cx).and_then(|(job, running)| {
            let conflict = running
                .asking
                .as_ref()
                .filter(|asked| asks_password(asked))?;
            Some((job, conflict.number))
        });
        let Some((job, number)) = asked.filter(|_| !self.picker()) else {
            self.island.password = None;
            return;
        };
        let shown = self.island.password.as_ref();
        if shown.is_some_and(|shown| shown.job == job && shown.number == number) {
            return;
        }
        let field = cx.new(|cx| TextField::new(cx).hiding());
        let events = cx.subscribe_in(&field, window, |this, _, event, _, cx| match event {
            TextFieldEvent::Submitted => this.give_password(cx),
            TextFieldEvent::Cancelled => this.cancel_job(cx),
            TextFieldEvent::Changed => {}
        });
        window.focus(&field.read(cx).focus_handle(cx), cx);
        self.island.password = Some(Password {
            job,
            number,
            field,
            _events: events,
        });
    }

    pub(crate) fn give_password(&self, cx: &mut Context<Self>) {
        let asked = self
            .job_on_island(cx)
            .and_then(|(job, running)| Some((job, running.asking.clone()?)));
        let (Some((job, conflict)), Some(shown), Some(connection)) =
            (asked, &self.island.password, &self.connection)
        else {
            return;
        };
        let password = shown.field.read(cx).text().to_owned();
        if shown.job != job || shown.number != conflict.number || password.is_empty() {
            return;
        }
        connection
            .send(Command::GivePassword {
                job,
                conflict,
                password,
            })
            .ok();
        shown.field.update(cx, |field, cx| field.set_text("", cx));
        cx.notify();
    }
}
