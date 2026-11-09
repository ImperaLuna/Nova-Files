//! How a picker window opens and closes: over the asking app's window,
//! when the app names one, at the size the last picker closed at (see
//! `window_state`), which a closing picker saves.

use gpui::{Context, ForeignParent, Pixels, Size, Subscription, Window};
use nova_files_api::{Command, ParentWindow};

use super::Picker;
use crate::window::MainWindow;
use crate::window_state::WindowState;

/// The asking app's window as GPUI names another program's window, for
/// the picker to open over.
pub fn foreign_parent(parent: &ParentWindow) -> ForeignParent {
    match parent {
        ParentWindow::Wayland(handle) => ForeignParent::Wayland(handle.clone()),
        ParentWindow::X11(id) => ForeignParent::X11(*id),
    }
}

/// The window's size now, and a subscription that keeps the picker's
/// copy of it as it changes. A maximized window's is the size it goes
/// back to.
pub(super) fn watch_size(
    window: &mut Window,
    cx: &Context<MainWindow>,
) -> (Size<Pixels>, Subscription) {
    let bounds = cx.observe_window_bounds(window, |this, window, _| {
        if let Some(pick) = &mut this.pick {
            pick.size = window.window_bounds().get_bounds().size;
        }
    });
    (window.window_bounds().get_bounds().size, bounds)
}

impl MainWindow {
    /// The windows' state as a picker opens, which its size joins when it
    /// closes.
    pub fn set_pick_window_state(&mut self, state: WindowState) {
        if let Some(pick) = &mut self.pick {
            pick.window_state = state;
        }
    }

    /// Ends pick mode and saves the picker's size for the next one. The
    /// caller answers the request, or drops it to answer cancelled.
    pub(super) fn take_pick(&mut self) -> Option<Picker> {
        let pick = self.pick.take()?;
        let state = pick.window_state.clone().with_picker(pick.size);
        if let Some(connection) = &self.connection {
            // A core that is gone keeps nothing anyway.
            connection
                .send(Command::SaveWindowState(state.to_bytes()))
                .ok();
        }
        Some(pick)
    }
}

impl Drop for MainWindow {
    /// A picker closed by its window, not its buttons, still saves its
    /// size; its request answers cancelled as it is dropped.
    fn drop(&mut self) {
        drop(self.take_pick());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picker_opens_over_the_window_the_app_names() {
        assert_eq!(
            foreign_parent(&ParentWindow::Wayland("2a9c".to_owned())),
            ForeignParent::Wayland("2a9c".to_owned())
        );
        assert_eq!(foreign_parent(&ParentWindow::X11(7)), ForeignParent::X11(7));
    }
}
