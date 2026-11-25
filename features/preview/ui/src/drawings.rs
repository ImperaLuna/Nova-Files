use std::path::PathBuf;
use std::sync::Arc;

use gpui::Context;
use nova_files_api::{Fit, Picture};
use nova_files_preview_api::{Command, PreviewId};

use crate::looks::render_image;
use nova_files_shell::device::device_side;

use crate::page::{PICTURE_ROOM, PICTURE_TALL, Page};
use crate::{PreviewPart, release};

const PERCENT: f32 = 100.0;
const MAX_ZOOM_PERCENT: f32 = 400.0;

impl PreviewPart {
    pub(crate) fn ask_pictures(&self, id: PreviewId) {
        let Some(open) = self.looks.open.as_ref().filter(|open| open.asked == id) else {
            return;
        };
        let scale_factor = self
            .looks
            .room
            .map_or(1.0, |(_, scale_factor)| scale_factor);
        let width = device_side(PICTURE_ROOM, scale_factor);
        let fit = Fit {
            width,
            height: width.saturating_mul(PICTURE_TALL),
        };
        let Some(shown) = open.current() else {
            return;
        };
        let (Some(page), Some(preview)) = (&shown.page, &shown.preview) else {
            return;
        };
        let zoom_percent = zoom_percent(scale_factor);
        for (kind, source) in page.pictures.figuring() {
            let figure = Command::Figure {
                id,
                kind: *kind,
                source: Arc::clone(source),
                zoom_percent,
                fit,
            };
            self.host.link.send(figure).ok();
        }
        for picture in page.pictures.asking() {
            let draw = Command::Draw {
                id,
                document: preview.path.clone(),
                picture: picture.clone(),
                fit,
            };
            self.host.link.send(draw).ok();
        }
    }

    pub(crate) fn drawn(
        &self,
        id: PreviewId,
        picture: PathBuf,
        drawn: Option<Picture>,
        cx: &Context<Self>,
    ) {
        let Some(drawn) = drawn.filter(|_| self.page_of(id).is_some()) else {
            return;
        };
        let converting = cx
            .background_executor()
            .spawn(async move { render_image(&drawn) });
        cx.spawn(async move |this, cx| {
            let Some(image) = converting.await.map(Arc::new) else {
                return;
            };
            this.update(cx, |this, cx| {
                let released = match this.page_of_mut(id) {
                    Some(page) => {
                        page.list.remeasure();
                        page.pictures.arrived(&picture, &image)
                    }
                    None => vec![image],
                };
                release(released, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn figured(
        &self,
        id: PreviewId,
        source: Arc<str>,
        drawn: Option<Picture>,
        cx: &Context<Self>,
    ) {
        let Some(drawn) = drawn.filter(|_| self.page_of(id).is_some()) else {
            return;
        };
        let converting = cx
            .background_executor()
            .spawn(async move { render_image(&drawn) });
        cx.spawn(async move |this, cx| {
            let Some(image) = converting.await.map(Arc::new) else {
                return;
            };
            this.update(cx, |this, cx| {
                let released = match this.page_of_mut(id) {
                    Some(page) => {
                        page.list.remeasure();
                        page.pictures.figured(&source, image)
                    }
                    None => Some(image),
                };
                release(released, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn page_of(&self, id: PreviewId) -> Option<&Page> {
        let open = self.looks.open.as_ref().filter(|open| open.asked == id)?;
        open.current()?.page.as_ref()
    }

    fn page_of_mut(&mut self, id: PreviewId) -> Option<&mut Page> {
        let open = self.looks.open.as_mut().filter(|open| open.asked == id)?;
        let entry = open.entry;
        open.shown
            .as_mut()
            .filter(|shown| shown.entry == entry)?
            .page
            .as_mut()
    }
}

fn zoom_percent(scale_factor: f32) -> u16 {
    let percent = (scale_factor * PERCENT)
        .round()
        .clamp(PERCENT, MAX_ZOOM_PERCENT);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped between 100 and 400 first"
    )]
    let percent = percent as u16;
    percent
}
