use gpui::{Context, ForeignParent, Window};
use nova_files_api::Command;
use nova_files_picker_api::ParentWindow;

use crate::window::MainWindow;
use crate::window_state::WindowState;

pub fn foreign_parent(parent: &ParentWindow) -> ForeignParent {
    match parent {
        ParentWindow::Wayland(handle) => ForeignParent::Wayland(handle.clone()),
        ParentWindow::X11(id) => ForeignParent::X11(*id),
    }
}

impl MainWindow {
    pub(super) fn watch_pick_size(&mut self, window: &mut Window, cx: &Context<Self>) {
        self.pick_size = Some(window.window_bounds().get_bounds().size);
        self.pick_bounds = Some(cx.observe_window_bounds(window, |this, window, _| {
            this.pick_size = Some(window.window_bounds().get_bounds().size);
        }));
    }

    pub(crate) fn save_pick_size(&mut self) {
        let Some(size) = self.pick_size.take() else {
            return;
        };
        let state = WindowState::default().with_picker(size);
        if let Some(connection) = &self.connection {
            connection
                .send(Command::SaveWindowState(state.to_bytes()))
                .ok();
        }
    }
}

impl Drop for MainWindow {
    fn drop(&mut self) {
        self.save_pick_size();
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
