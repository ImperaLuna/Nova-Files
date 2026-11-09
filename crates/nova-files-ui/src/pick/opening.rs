//! How a picker window opens: over the asking app's window, when the app
//! names one.

use gpui::ForeignParent;
use nova_files_api::ParentWindow;

/// The asking app's window as GPUI names another program's window, for
/// the picker to open over.
pub fn foreign_parent(parent: &ParentWindow) -> ForeignParent {
    match parent {
        ParentWindow::Wayland(handle) => ForeignParent::Wayland(handle.clone()),
        ParentWindow::X11(id) => ForeignParent::X11(*id),
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
