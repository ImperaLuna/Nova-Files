use gpui::{Context, ForeignParent, Pixels, Size, Subscription, Window};
use nova_files_api::Command;
use nova_files_picker_api::ParentWindow;

use super::Picker;
use crate::window::MainWindow;
use crate::window_state::WindowState;

pub fn foreign_parent(parent: &ParentWindow) -> ForeignParent {
    match parent {
        ParentWindow::Wayland(handle) => ForeignParent::Wayland(handle.clone()),
        ParentWindow::X11(id) => ForeignParent::X11(*id),
    }
}

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
    pub(super) fn take_pick(&mut self) -> Option<Picker> {
        let pick = self.pick.take()?;
        let state = WindowState::default().with_picker(pick.size);
        if let Some(connection) = &self.connection {
            connection
                .send(Command::SaveWindowState(state.to_bytes()))
                .ok();
        }
        Some(pick)
    }
}

impl Drop for MainWindow {
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
