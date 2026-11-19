use std::os::unix::fs::PermissionsExt as _;

use nova_files_properties_ui::{AccessLevel, Who, shared_level};
use nova_files_ui::Look;

use super::*;

fn mode_of(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().mode() & 0o7777
}

async fn shows_mode(view: &Entity<MainWindow>, cx: &VisualTestContext, mode: u32) {
    view.condition::<WindowEvent>(cx, |view, cx| {
        props(view, cx)
            .page()
            .and_then(|page| page.read().first().map(|one| one.mode))
            == Some(mode)
    })
    .await;
}

fn pick_level(
    view: &Entity<MainWindow>,
    cx: &mut VisualTestContext,
    chip: &'static str,
    level: AccessLevel,
) {
    cx.run_until_parked();
    let bounds = cx.debug_bounds(chip).unwrap();
    cx.simulate_mouse_down(bounds.center(), gpui::MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    let who = Who::ALL[chip["access-".len()..].parse::<usize>().unwrap()];
    let index = view.read_with(cx, |view, cx| {
        let page = props(view, cx).page().unwrap().read();
        let items = &view.menu().unwrap().items;
        let on: Vec<&str> = items
            .iter()
            .filter(|item| item.look == Look::On)
            .map(|item| item.label.as_ref())
            .collect();
        let now = shared_level(&page, who).unwrap();
        assert_eq!(on, [now.label()]);
        items
            .iter()
            .position(|item| item.label == level.label())
            .unwrap()
    });
    click(cx, ["menu-0", "menu-1", "menu-2"][index]);
}

#[gpui::test]
async fn a_level_chosen_changes_the_disk_and_shows(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("properties-access");
    let notes = tree.path().join("notes.txt");
    fs::set_permissions(&notes, fs::Permissions::from_mode(0o600)).unwrap();
    let (view, cx) = properties_window(cx, tree.path()).await;
    click(cx, "name-1");
    show_properties(&view, cx);
    read_page(&view, cx).await;
    assert!(view.read_with(cx, |view, cx| props(view, cx).may_change_access()));

    pick_level(&view, cx, "access-1", AccessLevel::ReadOnly);
    shows_mode(&view, cx, 0o640).await;
    assert_eq!(mode_of(&notes), 0o640);
    pick_level(&view, cx, "access-0", AccessLevel::ReadOnly);
    shows_mode(&view, cx, 0o440).await;

    click(cx, "access-runs");
    shows_mode(&view, cx, 0o550).await;
    click(cx, "access-runs");
    shows_mode(&view, cx, 0o440).await;

    view.update(cx, MainWindow::close_current_tab);
    click(cx, "name-1");
    show_properties(&view, cx);
    assert_eq!(read_page(&view, cx).await[0].mode, 0o440);
    assert_eq!(
        view.read_with(cx, |view, cx| {
            let page = props(view, cx).page().unwrap().read();
            shared_level(&page, Who::Others)
        }),
        Some(AccessLevel::NoAccess)
    );
}

#[gpui::test]
async fn a_folder_reaches_inside_only_when_ticked(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("properties-access-inside");
    let projects = tree.path().join("Projects");
    let nova = projects.join("nova");
    fs::set_permissions(&projects, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&nova, fs::Permissions::from_mode(0o755)).unwrap();
    let (view, cx) = properties_window(cx, tree.path()).await;
    click(cx, "name-0");
    show_properties(&view, cx);
    read_page(&view, cx).await;

    pick_level(&view, cx, "access-2", AccessLevel::NoAccess);
    shows_mode(&view, cx, 0o750).await;
    assert_eq!(mode_of(&nova), 0o755);

    assert!(!view.read_with(cx, |view, cx| props(view, cx).reaches_inside()));
    click(cx, "access-inside");
    assert!(view.read_with(cx, |view, cx| props(view, cx).reaches_inside()));
    pick_level(&view, cx, "access-1", AccessLevel::NoAccess);
    shows_mode(&view, cx, 0o700).await;
    assert_eq!(mode_of(&nova), 0o705);
}

#[gpui::test]
async fn several_files_offer_neither_tick(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("properties-access-files");
    let files = ["a.txt", "b.txt"].map(|name| tree.path().join(name));
    for file in &files {
        fs::write(file, "").unwrap();
    }
    let (view, cx) = properties_window(cx, tree.path()).await;
    let place = properties_at(files.to_vec());
    view.update(cx, |view, cx| view.new_tab(&place, cx));
    assert_eq!(read_page(&view, cx).await.len(), 2);
    assert!(cx.debug_bounds("access-runs").is_none());
    assert!(cx.debug_bounds("access-inside").is_none());
}

#[gpui::test]
async fn what_the_user_does_not_own_offers_no_changes(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("properties-access-not-own");
    let root = Path::new("/");
    let (view, cx) = properties_window(cx, tree.path()).await;
    let place = properties_at(vec![root.to_owned()]);
    view.update(cx, |view, cx| view.new_tab(&place, cx));
    read_page(&view, cx).await;
    let own = fs::metadata(tree.path()).unwrap().uid() == fs::metadata(root).unwrap().uid();
    assert_eq!(
        view.read_with(cx, |view, cx| props(view, cx).may_change_access()),
        own
    );
    if !own {
        click(cx, "access-0");
        assert!(view.read_with(cx, |view, _| view.menu().is_none()));
    }
}

#[gpui::test]
async fn a_drive_mounted_read_only_offers_no_changes(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let Some(read_only) = read_only_mount() else {
        return;
    };
    let tree = TempFolder::tree("properties-access-read-only");
    let (view, cx) = properties_window(cx, tree.path()).await;
    let place = properties_at(vec![read_only]);
    view.update(cx, |view, cx| view.new_tab(&place, cx));
    let read = read_page(&view, cx).await;
    assert_eq!(read[0].read_only, Some(true));
    assert!(!view.read_with(cx, |view, cx| props(view, cx).may_change_access()));
}

fn read_only_mount() -> Option<std::path::PathBuf> {
    let mounts = fs::read_to_string("/proc/self/mounts").ok()?;
    mounts.lines().find_map(|line| {
        let mut fields = line.split(' ');
        let folder = Path::new(fields.nth(1)?);
        let read_only = fields.nth(1)?.split(',').any(|option| option == "ro");
        (read_only && folder.is_dir() && fs::read_dir(folder).is_ok()).then(|| folder.to_owned())
    })
}

#[gpui::test]
async fn the_island_shows_on_the_page(cx: &mut TestAppContext) {
    let _watchdog = Watchdog::start();
    let tree = TempFolder::tree("properties-island");
    let (view, cx) = properties_window(cx, tree.path()).await;
    click(cx, "name-1");
    cx.simulate_keystrokes("delete");
    view.condition::<WindowEvent>(cx, |view, _| view.note().is_some())
        .await;

    click(cx, "name-0");
    show_properties(&view, cx);
    read_page(&view, cx).await;
    assert!(view.read_with(cx, |view, _| view.note().is_some()));
    assert!(cx.debug_bounds("island-undo").is_some());
    assert!(cx.debug_bounds("count").is_none());
}
