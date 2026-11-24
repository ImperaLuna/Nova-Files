use gpui::{
    AnyElement, AppContext as _, ClickEvent, Context, Entity, Focusable as _, IntoElement as _,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
};
use nova_files_api::{Answer, Carried, Command, Conflict, ConflictKind, JobId, Question, Scope};
use nova_files_archives_api::{PASSWORD_TO_LOCK, PASSWORD_WRONG, Password};
use nova_files_shell::island::asking;
use nova_files_shell::island::look::{floating, label, password_box, worded};
use nova_files_shell::island::words::name_of;
use nova_files_shell::text::{TextField, TextFieldEvent};
use nova_files_shell::theme::{Colors, text};
use nova_files_shell::{Asking, Scene};

use crate::ArchivesPart;

const UNLOCK: &str = "Unlock";
const LOCK: &str = "Lock";
const CANCEL: &str = "Cancel";
const NEEDED: &str = ": needs a password";
const WRONG: &str = ": wrong password";

pub struct Asked {
    job: JobId,
    conflict: Conflict,
    field: Entity<TextField>,
    _events: Subscription,
}

impl ArchivesPart {
    pub(crate) fn asks(
        &mut self,
        asking: Option<Asking<'_>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(asking) = asking else {
            self.drop_password(window, cx);
            return;
        };
        let shown = self.password.as_ref();
        if shown.is_some_and(|shown| {
            shown.job == asking.job && shown.conflict.number == asking.conflict.number
        }) {
            return;
        }
        let field = cx.new(|cx| TextField::new(cx).hiding());
        let events = cx.subscribe_in(&field, window, |part, _, event, _, cx| match event {
            TextFieldEvent::Submitted => part.give_password(cx),
            TextFieldEvent::Cancelled => part.cancel_asked(),
            TextFieldEvent::Changed => {}
        });
        if window
            .focused(cx)
            .is_none_or(|focused| focused == self.host.list)
        {
            window.focus(&field.read(cx).focus_handle(cx), cx);
        }
        self.password = Some(Asked {
            job: asking.job,
            conflict: asking.conflict.clone(),
            field,
            _events: events,
        });
        cx.notify();
    }

    fn drop_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(shown) = self.password.take() else {
            return;
        };
        if shown.field.read(cx).focus_handle(cx).is_focused(window) {
            window.focus(&self.host.list, cx);
        }
        cx.notify();
    }

    fn give_password(&self, cx: &mut Context<Self>) {
        let Some(shown) = &self.password else {
            return;
        };
        let password = shown.field.read(cx).text().to_owned();
        if password.is_empty() {
            return;
        }
        self.host
            .link
            .send(Command::Reply {
                job: shown.job,
                conflict: shown.conflict.clone(),
                reply: Carried::new(Password(password)),
            })
            .ok();
        shown.field.update(cx, |field, cx| field.set_text("", cx));
    }

    fn skip_asked(&self) {
        if let Some(shown) = &self.password {
            self.host
                .link
                .send(Command::AnswerConflict {
                    job: shown.job,
                    conflict: shown.conflict.clone(),
                    answer: Answer::Skip,
                    scope: Scope::ThisOne,
                })
                .ok();
        }
    }

    fn cancel_asked(&self) {
        if let Some(shown) = &self.password {
            self.host.link.send(Command::CancelJob(shown.job)).ok();
        }
    }
}

pub fn island(
    part: &mut ArchivesPart,
    _: &Scene,
    _: &mut Window,
    cx: &mut Context<ArchivesPart>,
) -> Option<AnyElement> {
    let shown = part.password.as_ref()?;
    let colors = Colors::of(cx);
    let ConflictKind::Asked(question) = shown.conflict.kind else {
        return None;
    };
    let parts = vec![
        label(&colors)
            .child(
                div()
                    .text_color(colors.text)
                    .font_weight(text::STRONG_WEIGHT)
                    .child(name_of(&shown.conflict.source)),
            )
            .child(reason(question))
            .into_any_element(),
        password_box(shown.field.clone(), &colors).into_any_element(),
        worded("island-cancel", CANCEL, &colors)
            .on_click(cx.listener(|part, _: &ClickEvent, _, _| part.cancel_asked()))
            .into_any_element(),
        worded("island-skip", asking::label(Answer::Skip), &colors)
            .on_click(cx.listener(|part, _: &ClickEvent, _, _| part.skip_asked()))
            .into_any_element(),
        worded("island-unlock", accept(question), &colors)
            .text_color(colors.accept)
            .on_click(cx.listener(|part, _: &ClickEvent, _, cx| part.give_password(cx)))
            .into_any_element(),
    ];
    Some(floating(parts, None, &colors).into_any_element())
}

fn reason(question: &'static Question) -> &'static str {
    if *question == PASSWORD_WRONG {
        WRONG
    } else {
        NEEDED
    }
}

fn accept(question: &'static Question) -> &'static str {
    if *question == PASSWORD_TO_LOCK {
        LOCK
    } else {
        UNLOCK
    }
}

#[cfg(test)]
mod tests {
    use nova_files_archives_api::PASSWORD_NEEDED;

    use super::*;

    #[test]
    fn only_a_compress_locks_and_only_a_wrong_password_says_so() {
        assert_eq!(accept(&PASSWORD_TO_LOCK), LOCK);
        assert_eq!(accept(&PASSWORD_NEEDED), UNLOCK);
        assert_eq!(accept(&PASSWORD_WRONG), UNLOCK);
        assert_eq!(reason(&PASSWORD_WRONG), WRONG);
        assert_eq!(reason(&PASSWORD_NEEDED), NEEDED);
        assert_eq!(reason(&PASSWORD_TO_LOCK), NEEDED);
    }
}
