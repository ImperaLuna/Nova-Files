use std::sync::Arc;
use std::time::Duration;

use gpui::{Context, KeyDownEvent};
use nova_files_preview_api::{Command, Control, Played, PreviewId};
use nova_files_shell::Scene;

use crate::looks::render_image;
use crate::player::{Media, State};
use crate::{PreviewPart, release};

const PAUSE_KEY: &str = "k";
const BACK_KEY: &str = "j";
const FORWARD_KEY: &str = "l";

impl PreviewPart {
    pub(crate) fn shown_media(&mut self) -> Option<&mut Media> {
        let open = self.looks.open.as_mut()?;
        let (entry, asked) = (open.entry, open.asked);
        open.shown
            .as_mut()
            .filter(|shown| shown.entry == entry)?
            .media
            .as_mut()
            .filter(|media| media.id == asked)
    }

    pub(crate) fn control(&self, id: PreviewId, control: Control) {
        self.host.link.send(Command::Control { id, control }).ok();
    }

    pub(crate) fn start_playing(&mut self) {
        let volume = self.looks.volume;
        if let Some(media) = self.shown_media().map(|media| media.id) {
            self.control(media, Control::Volume(volume));
            self.control(media, Control::Play);
        }
    }

    pub(crate) fn play_or_pause(&mut self, cx: &mut Context<Self>) {
        let Some(media) = self.shown_media() else {
            return;
        };
        let id = media.id;
        let controls: &[Control] = match media.state {
            State::Ended => &[Control::Seek(Duration::ZERO), Control::Play],
            State::Paused => &[Control::Play],
            State::Playing => &[Control::Pause],
            State::Unplayable => &[],
        };
        for control in controls {
            self.control(id, *control);
        }
        cx.notify();
    }

    pub(crate) fn seek_step(&mut self, step: u16, cx: &mut Context<Self>) {
        let Some(media) = self.shown_media() else {
            return;
        };
        let id = media.id;
        if let Some(at) = media.at_step(step) {
            media.moved_to(at);
            self.control(id, Control::Seek(at));
            cx.notify();
        }
    }

    pub(crate) fn let_go_of_seek(&mut self) {
        if let Some(media) = self.shown_media() {
            media.let_go();
        }
    }

    pub(crate) fn set_volume(&mut self, volume: u8, cx: &mut Context<Self>) {
        self.looks.volume = volume;
        if let Some(media) = self.shown_media().map(|media| media.id) {
            self.control(media, Control::Volume(volume));
        }
        cx.notify();
    }

    pub(crate) fn jump(&mut self, back: bool) {
        let Some(media) = self.shown_media() else {
            return;
        };
        let (id, at) = (media.id, media.jumped(back));
        media.moved_to(at);
        self.control(id, Control::Seek(at));
    }

    pub(crate) fn key_down(
        &mut self,
        event: &KeyDownEvent,
        _: &Scene,
        cx: &mut Context<Self>,
    ) -> bool {
        let held = event.keystroke.modifiers;
        let plain = !(held.control || held.alt || held.platform || held.function || held.shift);
        if !self.previewing() || !plain || self.shown_media().is_none() {
            return false;
        }
        match event.keystroke.key.as_str() {
            PAUSE_KEY => self.play_or_pause(cx),
            BACK_KEY => self.jump(true),
            FORWARD_KEY => self.jump(false),
            _ => return false,
        }
        cx.notify();
        true
    }

    pub(crate) fn played(&mut self, id: PreviewId, played: Played, cx: &mut Context<Self>) {
        let Some(media) = self.shown_media().filter(|media| media.id == id) else {
            return;
        };
        let Played::Frame(picture) = played else {
            media.heard(played);
            cx.notify();
            return;
        };
        let converting = cx
            .background_executor()
            .spawn(async move { render_image(&picture) });
        cx.spawn(async move |this, cx| {
            let image = converting.await.map(Arc::new);
            this.update(cx, |this, cx| {
                let unused = match this.shown_media() {
                    Some(media) if media.id == id => {
                        image.and_then(|image| media.frame.replace(image))
                    }
                    _ => image,
                };
                release(unused, cx);
                this.control(id, Control::Drawn);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
