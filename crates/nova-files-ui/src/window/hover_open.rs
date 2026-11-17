//! Hover to switch: files held over a tab in the Open list for
//! [`HOVER_OPEN_AFTER`] show that tab, and over a place, a drive or a tree
//! folder open it in the tab on screen, so the drag can go on to a folder
//! inside. The drag stays alive throughout. Leaving the row sooner opens
//! nothing, and a row is opened once per stay over it. A tab on Start,
//! which takes no files, still switches. It works for files from another
//! app as well as from a list.
//!
//! Each move of the pointer while something is dragged is told first to
//! the rows under it, innermost first, and the first one claims the move
//! (see `view::drop`), then to the window's root, which sees which row,
//! if any, the pointer is over now: a new one starts the wait, none drops
//! it.

use std::path::Path;
use std::time::Duration;

use gpui::{Bounds, Context, Point, Task, Window};

use super::MainWindow;
use crate::location::Location;

/// How long files are held over a row before it opens.
pub const HOVER_OPEN_AFTER: Duration = Duration::from_millis(700);

/// What a row opens when files are held over it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoverOpens {
    /// The tab at this place in the Open list.
    Tab(usize),
    /// A place, in the tab on screen.
    Place(Location),
}

impl HoverOpens {
    /// Whether it is the folder on screen, `here`, and so opens nothing.
    /// A search started from the folder shows what it found, not the
    /// folder: `searching`, it opens.
    fn on_screen(&self, here: Option<&Path>, searching: bool) -> bool {
        match self {
            Self::Place(Location::Folder(folder)) => here == Some(folder.as_path()) && !searching,
            Self::Tab(_) | Self::Place(Location::Start) => false,
        }
    }
}

/// The row the pointer is over while something is dragged.
#[derive(Default)]
pub struct HoverOpen {
    /// The row that claimed the move under way, innermost first.
    claimed: Option<HoverOpens>,
    /// The row the pointer stays over, and the wait before it opens; no
    /// wait once it opened.
    over: Option<(HoverOpens, Option<Task<()>>)>,
}

impl MainWindow {
    /// The pointer moved over a row that opens `opens` while something is
    /// dragged; a row nearer the pointer may have claimed the move first.
    pub(crate) fn hovered_while_dragging(&mut self, opens: HoverOpens) {
        self.hover_open.claimed.get_or_insert(opens);
    }

    /// The pointer moved over the window, `dragging` or not: after the
    /// rows under it heard it (see [`Self::hovered_while_dragging`]).
    pub(crate) fn pointer_moved(&mut self, dragging: bool, window: &Window, cx: &Context<Self>) {
        let claimed = self.hover_open.claimed.take().filter(|_| dragging);
        let same = self.hover_open.over.as_ref().map(|(opens, _)| opens) == claimed.as_ref();
        if same {
            return;
        }
        self.hover_open.over = claimed.map(|opens| {
            let wait = cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(HOVER_OPEN_AFTER).await;
                // A window closed opens nothing.
                this.update_in(cx, Self::hover_open_due).ok();
            });
            (opens, Some(wait))
        });
    }

    /// The wait over a row is over: it opens, if something is still
    /// dragged and the pointer is still in the window. A drag that held
    /// the pointer as it left goes on being told where it is, outside, but
    /// no row hears that.
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
            // Start shown again is Start as it was.
            HoverOpens::Place(Location::Start) => self.open_start(cx),
            HoverOpens::Place(Location::Folder(folder)) => {
                self.open(&folder);
                cx.notify();
            }
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
        assert!(!HoverOpens::Place(Location::Start).on_screen(None, false));
    }
}
