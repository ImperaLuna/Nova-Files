use gpui::{Context, Pixels, Point};
use nova_files_shell::RowItem;

use super::MainWindow;
use crate::menu::MenuItem;

impl MainWindow {
    pub(crate) fn open_row_menu(&mut self, row: &str, at: Point<Pixels>, cx: &mut Context<Self>) {
        let groups = self.parts.row_menu(row, cx);
        self.open_part_menu(at, groups, cx);
    }

    pub(crate) fn open_part_menu(
        &mut self,
        at: Point<Pixels>,
        groups: Vec<Vec<RowItem>>,
        cx: &mut Context<Self>,
    ) {
        let groups: Vec<Vec<MenuItem>> = groups
            .into_iter()
            .map(|group| group.into_iter().map(MenuItem::from).collect())
            .collect();
        self.open_menu_groups(at, groups, false, cx);
    }
}
