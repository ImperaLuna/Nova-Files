mod gallery;
mod icons;

use nova_files_folders_api::{
    Command as ViewsCommand, Event as ViewsEvent, FolderView, IconSize, ViewMode,
};

use super::*;

fn shown(cx: &mut TestAppContext) -> (Entity<MainWindow>, &mut VisualTestContext, Core) {
    cx.update(nova_files_ui::bind_window_keys);
    let (view, cx, mut core) = window(cx);
    answer_listing(&mut core, ENTRIES);
    cx.run_until_parked();
    core.commands();
    (view, cx, core)
}

fn views_set(core: &mut Core) -> Vec<(PathBuf, FolderView)> {
    core.sent::<ViewsCommand>()
        .into_iter()
        .filter_map(|command| match command {
            ViewsCommand::SetView { folder, view } => Some((folder, view)),
            ViewsCommand::Views => None,
        })
        .collect()
}

fn showing(view: &Entity<MainWindow>, cx: &VisualTestContext) -> FolderView {
    view.read_with(cx, MainWindow::folder_view)
}

const fn icons(size: IconSize) -> FolderView {
    FolderView {
        mode: ViewMode::Icons,
        icons: size,
    }
}

#[gpui::test]
fn ctrl_numbers_pick_the_view_and_the_core_keeps_it(cx: &mut TestAppContext) {
    let (view, cx, mut core) = shown(cx);
    assert_eq!(showing(&view, cx), FolderView::default());

    cx.simulate_keystrokes("ctrl-2");
    let medium = icons(IconSize::Medium);
    assert_eq!(showing(&view, cx), medium);
    assert_eq!(views_set(&mut core), [(PathBuf::from(SHOWN), medium)]);

    cx.simulate_keystrokes("ctrl-3");
    assert_eq!(showing(&view, cx).mode, ViewMode::Gallery);
    cx.simulate_keystrokes("ctrl-1");
    assert_eq!(showing(&view, cx).mode, ViewMode::Details);
    let modes: Vec<ViewMode> = views_set(&mut core)
        .into_iter()
        .map(|(_, view)| view.mode)
        .collect();
    assert_eq!(modes, [ViewMode::Gallery, ViewMode::Details]);

    cx.simulate_keystrokes("ctrl-1");
    assert_eq!(views_set(&mut core), []);
}

#[gpui::test]
fn ctrl_plus_and_minus_step_the_icons_only_while_icons_show(cx: &mut TestAppContext) {
    let (view, cx, mut core) = shown(cx);

    cx.simulate_keystrokes("ctrl-=");
    assert_eq!(showing(&view, cx), FolderView::default());
    assert_eq!(views_set(&mut core), []);

    cx.simulate_keystrokes("ctrl-2 ctrl-= ctrl-+");
    assert_eq!(showing(&view, cx), icons(IconSize::Largest));
    cx.simulate_keystrokes("ctrl--");
    assert_eq!(showing(&view, cx), icons(IconSize::Large));

    cx.simulate_keystrokes("ctrl-1 ctrl-2");
    assert_eq!(showing(&view, cx), icons(IconSize::Large));
}

#[gpui::test]
fn the_pictures_place_starts_in_icons(cx: &mut TestAppContext) {
    let (view, cx, core) = shown(cx);
    for (kind, mode) in [
        (PlaceKind::Documents, ViewMode::Details),
        (PlaceKind::Pictures, ViewMode::Icons),
        (PlaceKind::Videos, ViewMode::Icons),
    ] {
        core.send(places_api::Event::Places(vec![Place {
            kind,
            path: PathBuf::from(SHOWN),
        }]));
        cx.run_until_parked();
        assert_eq!(showing(&view, cx).mode, mode, "{kind:?}");
    }
    core.send(places_api::Event::Places(vec![Place {
        kind: PlaceKind::Pictures,
        path: Path::new(SHOWN).parent().unwrap().to_path_buf(),
    }]));
    cx.run_until_parked();
    assert_eq!(showing(&view, cx).mode, ViewMode::Icons);
}

#[gpui::test]
fn the_views_the_core_sends_win(cx: &mut TestAppContext) {
    let (view, cx, core) = shown(cx);
    let gallery = FolderView {
        mode: ViewMode::Gallery,
        icons: IconSize::Small,
    };

    core.send(ViewsEvent::Views(Arc::from([
        (PathBuf::from(OTHER), icons(IconSize::Large)),
        (PathBuf::from(SHOWN), gallery),
        (PathBuf::from(SHOWN), icons(IconSize::Small)),
    ])));
    cx.run_until_parked();
    assert_eq!(showing(&view, cx), gallery);

    core.send(ViewsEvent::Views(Arc::from([])));
    cx.run_until_parked();
    assert_eq!(showing(&view, cx), FolderView::default());
}

#[gpui::test]
fn a_change_the_core_hasnt_taken_yet_stays(cx: &mut TestAppContext) {
    let (view, cx, core) = shown(cx);
    let gallery = FolderView {
        mode: ViewMode::Gallery,
        icons: IconSize::Medium,
    };

    cx.simulate_keystrokes("ctrl-3");
    core.send(ViewsEvent::Views(Arc::from([])));
    cx.run_until_parked();
    assert_eq!(showing(&view, cx), gallery);

    core.send(ViewsEvent::Views(Arc::from([(
        PathBuf::from(SHOWN),
        gallery,
    )])));
    cx.run_until_parked();
    core.send(ViewsEvent::Views(Arc::from([])));
    cx.run_until_parked();
    assert_eq!(showing(&view, cx), FolderView::default());
}
