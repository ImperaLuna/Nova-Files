use gpui::{App, Entity};
use nova_files_shell::text::TextField;

use super::MainWindow;

impl MainWindow {
    pub fn rename_field(&self, index: usize, cx: &App) -> Option<Entity<TextField>> {
        let entry = self.listing.as_ref()?.entries.get(index)?;
        self.parts.name_field(entry, cx)
    }

    pub fn renaming(&self, cx: &App) -> bool {
        self.parts.busy(cx)
    }
}
