use gpui::{
    AnyElement, AppContext as _, ClickEvent, Context, Entity, Focusable as _, IntoElement as _,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    Window, div,
};
use nova_files_drives_api::{DriveAction, DriveFailure, DriveList, Password, VolumeId};
use nova_files_shell::island::look::{floating, label, password_box, worded};
use nova_files_shell::text::{TextField, TextFieldEvent};
use nova_files_shell::theme::{Colors, text};
use nova_files_shell::{Navigator, Scene};

use crate::DrivesPart;

const UNLOCK: &str = "Unlock";
const CANCEL: &str = "Cancel";
const NEEDED: &str = ": needs a password";
const WRONG: &str = ": wrong password";

pub struct Asked {
    volume: VolumeId,
    name: SharedString,
    wrong: bool,
    field: Entity<TextField>,
    _events: Subscription,
}

impl DrivesPart {
    pub(crate) fn ask_password(
        &mut self,
        volume: VolumeId,
        name: SharedString,
        cx: &mut Context<Self>,
    ) {
        if self
            .asked
            .as_ref()
            .is_some_and(|asked| asked.volume == volume)
        {
            return;
        }
        let field = cx.new(|cx| TextField::new(cx).hiding());
        let handle = field.read(cx).focus_handle(cx);
        self.host.nav.update(cx, |nav, cx| nav.focus(handle, cx));
        let events = cx.subscribe(&field, |part, _, event, cx| match event {
            TextFieldEvent::Submitted => part.give_password(cx),
            TextFieldEvent::Cancelled => part.stop_asking(cx),
            TextFieldEvent::Changed => {}
        });
        self.asked = Some(Asked {
            volume,
            name,
            wrong: false,
            field,
            _events: events,
        });
        cx.notify();
    }

    pub fn says_wrong_password(&self) -> bool {
        self.asked.as_ref().is_some_and(|asked| asked.wrong)
    }

    fn give_password(&mut self, cx: &mut Context<Self>) {
        let Some(asked) = &self.asked else {
            return;
        };
        let password = asked.field.read(cx).text().to_owned();
        if password.is_empty() {
            return;
        }
        self.opening = Some(asked.volume.clone());
        self.act(DriveAction::Unlock(
            asked.volume.clone(),
            Password(password),
        ));
        asked.field.update(cx, |field, cx| field.set_text("", cx));
    }

    pub(crate) fn stop_asking(&mut self, cx: &mut Context<Self>) {
        if self.asked.take().is_some() {
            self.host.nav.update(cx, Navigator::focus_list);
            cx.notify();
        }
    }

    pub(crate) fn unlock_done(
        &mut self,
        volume: &VolumeId,
        failure: Option<&DriveFailure>,
        cx: &mut Context<Self>,
    ) {
        let Some(asked) = self.asked.as_mut().filter(|asked| asked.volume == *volume) else {
            return;
        };
        if failure == Some(&DriveFailure::WrongPassword) {
            asked.wrong = true;
            cx.notify();
        } else {
            self.stop_asking(cx);
        }
    }

    pub(crate) fn stop_asking_unless_locked(&mut self, list: &DriveList, cx: &mut Context<Self>) {
        let still_locked = self.asked.as_ref().is_none_or(|asked| {
            list.volumes
                .iter()
                .any(|volume| volume.id == asked.volume && volume.locked)
        });
        let unlocking = self
            .asked
            .as_ref()
            .is_some_and(|asked| self.opening.as_ref() == Some(&asked.volume));
        if !still_locked && !unlocking {
            self.stop_asking(cx);
        }
    }
}

pub const fn asking(part: &DrivesPart, scene: &Scene, _: &gpui::App) -> bool {
    part.asked.is_some() && !scene.picker
}

pub fn island(
    part: &mut DrivesPart,
    _: &Scene,
    _: &mut Window,
    cx: &mut Context<DrivesPart>,
) -> Option<AnyElement> {
    let asked = part.asked.as_ref()?;
    let colors = Colors::of(cx);
    let parts = vec![
        label(&colors)
            .child(
                div()
                    .text_color(colors.text)
                    .font_weight(text::STRONG_WEIGHT)
                    .child(asked.name.clone()),
            )
            .child(reason(asked.wrong))
            .into_any_element(),
        password_box(asked.field.clone(), &colors).into_any_element(),
        worded("island-cancel", CANCEL, &colors)
            .on_click(cx.listener(|part, _: &ClickEvent, _, cx| part.stop_asking(cx)))
            .into_any_element(),
        worded("island-unlock", UNLOCK, &colors)
            .text_color(colors.accept)
            .on_click(cx.listener(|part, _: &ClickEvent, _, cx| part.give_password(cx)))
            .into_any_element(),
    ];
    Some(floating(parts, None, &colors).into_any_element())
}

const fn reason(wrong: bool) -> &'static str {
    if wrong { WRONG } else { NEEDED }
}
