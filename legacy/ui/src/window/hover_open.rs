use std::path::Path;
use std::time::Duration;

use gpui::{Bounds, Context, Point, Task, Window};

use super::MainWindow;
use nova_files_shell::Location;

pub const HOVER_OPEN_AFTER: Duration = Duration::from_millis(700);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoverOpens {
    Tab(usize),
    Place(Location),
}

impl HoverOpens {
    fn on_screen(&self, here: Option<&Path>, searching: bool) -> bool {
        match self {
            Self::Place(Location::Folder(folder)) => here == Some(folder.as_path()) && !searching,
            Self::Tab(_) | Self::Place(Location::Page(_)) => false,
        }
    }
}

#[derive(Default)]
pub struct HoverOpen {
    claimed: Option<HoverOpens>,
    over: Option<(HoverOpens, Option<Task<()>>)>,
}

impl MainWindow {
    pub(crate) fn hovered_while_dragging(&mut self, opens: HoverOpens) {
        self.hover_open.claimed.get_or_insert(opens);
    }

    pub(crate) fn pointer_moved(
        &mut self,
        dragging: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let tab = self.tab_drag_heard(cx);
        let claimed = self.hover_open.claimed.take().filter(|_| dragging && !tab);
        let same = self.hover_open.over.as_ref().map(|(opens, _)| opens) == claimed.as_ref();
        if same {
            return;
        }
        self.hover_open.over = claimed.map(|opens| {
            let wait = cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(HOVER_OPEN_AFTER).await;
                this.update_in(cx, Self::hover_open_due).ok();
            });
            (opens, Some(wait))
        });
    }

    fn hover_open_due(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((opens, wait)) = &mut self.hover_open.over else {
            return;
        };
        wait.take();
        let on_window = Bounds::new(Point::default(), window.viewport_size())
            .contains(&window.mouse_position());
        if !cx.has_active_drag() || !on_window {
            self.hover_open.over = None;
            return;
        }
        let opens = opens.clone();
        if opens.on_screen(self.folder(), self.searching()) {
            return;
        }
        match opens {
            HoverOpens::Tab(index) => self.switch_tab(index, cx),
            HoverOpens::Place(Location::Page(page)) if page.is_start() => self.open_start(cx),
            HoverOpens::Place(Location::Folder(folder)) => {
                self.open(&folder);
                cx.notify();
            }
            HoverOpens::Place(Location::Page(page)) => self.open_page(page, cx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_folder_on_screen_is_on_screen_and_not_while_searching() {
        let here = Path::new("/home/me");
        let folder = HoverOpens::Place(Location::Folder(here.to_path_buf()));
        assert!(folder.on_screen(Some(here), false));
        assert!(!folder.on_screen(Some(here), true));
        assert!(!folder.on_screen(Some(Path::new("/home")), false));
        assert!(!folder.on_screen(None, false));
        assert!(!HoverOpens::Tab(0).on_screen(Some(here), false));
        assert!(!HoverOpens::Place(Location::start()).on_screen(None, false));
    }
}
