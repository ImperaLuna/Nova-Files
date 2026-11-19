use std::path::{Path, PathBuf};

use gpui::{AppContext as _, Context, Entity, Focusable as _, Subscription, Window};
use nova_files_api::ListingId;

use super::crumbs::under_home;
use super::{PathBar, PathBarEvent};
use crate::paths::HOME;
use crate::text::{TextField, TextFieldEvent};
use crate::typed::{self, Typed};

pub(super) struct PathField {
    field: Entity<TextField>,
    from: PathBuf,
    shown: String,
    going: Option<ListingId>,
    _events: Subscription,
    _blur: Subscription,
}

pub fn shown_path(folder: &Path, home: &Path) -> String {
    match under_home(folder, home) {
        Some(rest) if rest.as_os_str().is_empty() => HOME.to_owned(),
        Some(rest) => format!("{HOME}/{}", rest.to_string_lossy()),
        None => folder.to_string_lossy().into_owned(),
    }
}

impl PathBar {
    pub fn field(&self) -> Option<&Entity<TextField>> {
        self.field.as_ref().map(|typing| &typing.field)
    }

    pub fn start_typing(&mut self, folder: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let shown = shown_path(folder, &self.host.home);
        self.menu = None;
        let field = cx.new(TextField::new);
        field.update(cx, |field, cx| {
            field.set_text(&shown, cx);
            field.select_range(0..shown.len(), cx);
        });
        let events = cx.subscribe(&field, |this, _, event, cx| match event {
            TextFieldEvent::Submitted => this.submit_typed(cx),
            TextFieldEvent::Cancelled => this.stop_typing(true, cx),
            TextFieldEvent::Changed => {}
        });
        let focus = field.read(cx).focus_handle(cx);
        let blur = cx.on_blur(&focus, window, |this, _, cx| {
            this.stop_typing(false, cx);
        });
        window.focus(&focus, cx);
        self.field = Some(PathField {
            field,
            from: folder.to_path_buf(),
            shown,
            going: None,
            _events: events,
            _blur: blur,
        });
        cx.notify();
    }

    pub fn stop_typing(&mut self, focus_back: bool, cx: &mut Context<Self>) {
        if self.field.take().is_some() && focus_back {
            cx.emit(PathBarEvent::Left);
        }
        cx.notify();
    }

    fn submit_typed(&mut self, cx: &mut Context<Self>) {
        let Some(typing) = &self.field else {
            return;
        };
        let text = typing.field.read(cx).text().to_owned();
        let target = if text == typing.shown {
            None
        } else {
            typed_target(&text, &self.host.home, &typing.from)
        };
        match target {
            Some(target) => cx.emit(PathBarEvent::Typed(target)),
            None => self.stop_typing(true, cx),
        }
    }

    pub fn going(&mut self, asked: Option<ListingId>, cx: &mut Context<Self>) {
        let Some(typing) = &mut self.field else {
            return;
        };
        typing.going = asked;
        if asked.is_none() {
            select_all(&typing.field, cx);
        }
    }

    pub fn arrived(&mut self, same_folder: bool, cx: &mut Context<Self>) {
        let ends = self
            .field
            .as_ref()
            .is_some_and(|typing| typing.going.is_some() || !same_folder);
        if ends {
            self.field = None;
            cx.notify();
        }
    }

    pub fn refused(&mut self, id: ListingId, cx: &mut Context<Self>) {
        let Some(typing) = &mut self.field else {
            return;
        };
        if typing.going != Some(id) {
            return;
        }
        typing.going = None;
        select_all(&typing.field, cx);
    }
}

fn select_all(field: &Entity<TextField>, cx: &mut Context<PathBar>) {
    field.update(cx, |field, cx| {
        let all = 0..field.text().len();
        field.select_range(all, cx);
    });
}

pub fn typed_target(text: &str, home: &Path, folder: &Path) -> Option<PathBuf> {
    match typed::read(text, home, folder) {
        Typed::Nothing => None,
        Typed::Folder(path) | Typed::Path(path) => Some(path),
        Typed::Name(name) => Some(folder.join(name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_under_home_shows_with_a_tilde() {
        let home = Path::new("/home/me");
        assert_eq!(shown_path(home, home), "~");
        assert_eq!(shown_path(Path::new("/home/me/a/b"), home), "~/a/b");
        assert_eq!(shown_path(Path::new("/home/meg"), home), "/home/meg");
        assert_eq!(shown_path(Path::new("/etc"), home), "/etc");
        assert_eq!(shown_path(Path::new("me/a"), Path::new("me")), "me/a");
        assert_eq!(shown_path(Path::new("/etc"), Path::new("/")), "/etc");
    }

    #[test]
    fn typed_text_goes_from_the_folder_on_screen_or_home() {
        let (home, folder) = (Path::new("/home/me"), Path::new("/home/me/a"));
        let target = |text| typed_target(text, home, folder);
        assert_eq!(target("~"), Some(PathBuf::from("/home/me")));
        assert_eq!(target("~/b"), Some(PathBuf::from("/home/me/b")));
        assert_eq!(target("/etc"), Some(PathBuf::from("/etc")));
        assert_eq!(target("c"), Some(PathBuf::from("/home/me/a/c")));
        assert_eq!(target("c/d/"), Some(PathBuf::from("/home/me/a/c/d/")));
        assert_eq!(target(" "), None);
    }
}
