use std::path::PathBuf;
use std::sync::Arc;

use gpui::Context;
use nova_files_api::{Fit, Picture};
use nova_files_preview_api::{Command, PreviewId};

use crate::looks::render_image;
use crate::page::Page;
use crate::{PreviewPart, release};

const PICTURE_FIT: Fit = Fit {
    width: 1600,
    height: 1600,
};

impl PreviewPart {
    pub(crate) fn ask_pictures(&self, id: PreviewId) {
        let Some(open) = self.looks.open.as_ref().filter(|open| open.asked == id) else {
            return;
        };
        let Some(shown) = open.current() else {
            return;
        };
        let (Some(page), Some(preview)) = (&shown.page, &shown.preview) else {
            return;
        };
        for picture in page.pictures.asking() {
            let draw = Command::Draw {
                id,
                document: preview.path.clone(),
                picture: picture.clone(),
                fit: PICTURE_FIT,
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
                    Some(page) => page.pictures.arrived(&picture, &image),
                    None => vec![image],
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
