use gpui::Pixels;
use nova_files_folders_api::{FolderView, IconSize, ViewMode};
use nova_files_shell::Layout;
use nova_files_shell::theme::size as sizes;

const STRIP_ICONS: IconSize = IconSize::Small;

pub const fn icon_side(icons: IconSize) -> Pixels {
    sizes::ICON_SIDES[icons.index()]
}

pub fn layout_of(view: FolderView) -> Layout {
    match view.mode {
        ViewMode::Details => Layout::Details,
        ViewMode::Icons => Layout::Icons {
            icon: icon_side(view.icons),
            beside: view.icons == IconSize::Smallest,
        },
        ViewMode::Gallery => Layout::Gallery {
            strip: icon_side(STRIP_ICONS),
        },
    }
}

#[cfg(test)]
mod tests {
    use gpui::px;

    use super::*;

    #[test]
    fn each_size_has_its_own_icon_side() {
        let sides: Vec<Pixels> = IconSize::ALL.into_iter().map(icon_side).collect();
        assert_eq!(sides, [px(48.0), px(64.0), px(96.0), px(128.0), px(192.0)]);
    }
}
