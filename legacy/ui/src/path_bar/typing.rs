use std::path::{Path, PathBuf};

use gpui::{AppContext as _, Context, Entity, Focusable as _, Subscription, Window};
use nova_files_api::ListingId;

use crate::crumbs::under_home;
use crate::pick::typed::{self, HOME, Typed};
use crate::text::{TextField, TextFieldEvent};
use crate::window::MainWindow;

pub struct PathField {
    pub(crate) field: Entity<TextField>,
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

impl MainWindow {
    pub fn path_field(&self) -> Option<&Entity<TextField>> {
        self.path_bar.field.as_ref().map(|typing| &typing.field)
    }

    pub(crate) fn start_typing_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(folder) = self.folder() else { return };
        let shown = shown_path(folder, &self.home);
        self.close_path_menu();
        let field = cx.new(TextField::new);
        field.update(cx, |field, cx| {
            field.set_text(&shown, cx);
            field.select_range(0..shown.len(), cx);
        });
        let events = cx.subscribe_in(&field, window, |this, _, event, window, cx| match event {
            TextFieldEvent::Submitted => this.submit_typed_path(window, cx),
            TextFieldEvent::Cancelled => this.stop_typing_path(Some(window), cx),
            TextFieldEvent::Changed => {}
        });
        let focus = field.read(cx).focus_handle(cx);
        let blur = cx.on_blur(&focus, window, |this, _, cx| {
            this.stop_typing_path(None, cx);
        });
        window.focus(&focus, cx);
        self.path_bar.field = Some(PathField {
            field,
            shown,
            going: None,
            _events: events,
            _blur: blur,
        });
        cx.notify();
    }

    pub(crate) fn stop_typing_path(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if self.path_bar.field.take().is_some()
            && let Some(window) = window
        {
            window.focus(&self.keys_home(cx), cx);
        }
        cx.notify();
    }

    fn submit_typed_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path_field) = &self.path_bar.field else {
            return;
        };
        let text = path_field.field.read(cx).text().to_owned();
        let target = if text == path_field.shown {
            None
        } else {
            self.folder()
                .and_then(|folder| typed_target(&text, &self.home, folder))
        };
        let Some(target) = target else {
            self.stop_typing_path(Some(window), cx);
            return;
        };
        let before = self.asked;
        self.open(&target);
        let asked = self.asked.filter(|_| self.asked != before);
        let Some(path_field) = &mut self.path_bar.field else {
            return;
        };
        path_field.going = asked;
        if asked.is_none() {
            select_all(&path_field.field, cx);
        }
    }

    pub(crate) fn typed_path_arrived(&mut self, same_folder: bool) {
        let ends = self
            .path_bar
            .field
            .as_ref()
            .is_some_and(|field| field.going.is_some() || !same_folder);
        if ends {
            self.path_bar.field = None;
        }
    }

    pub(crate) fn typed_path_refused(&mut self, id: ListingId, cx: &mut Context<Self>) {
        let Some(path_field) = &mut self.path_bar.field else {
            return;
        };
        if path_field.going != Some(id) {
            return;
        }
        path_field.going = None;
        select_all(&path_field.field, cx);
    }
}

fn select_all(field: &Entity<TextField>, cx: &mut Context<MainWindow>) {
    field.update(cx, |field, cx| {
        let all = 0..field.text().len();
        field.select_range(all, cx);
    });
}

fn typed_target(text: &str, home: &Path, folder: &Path) -> Option<PathBuf> {
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
